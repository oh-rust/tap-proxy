use clap::Parser;
use colored::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:8085", help = "Listen address")]
    pub listen: String,

    #[arg(
        short,
        help = "Destination server address, including hostname and port, e.g. example.com:80"
    )]
    pub dest: String,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Use TLS when connecting to the destination server"
    )]
    pub tls: bool,

    #[arg(
        short = 'z',
        long,
        default_value_t = true,
        help = "Strip 'Accept-Encoding' headers to prevent compressed responses"
    )]
    pub strip_compression: bool,
}

impl Args {
    fn domain(&self) -> String {
        self.dest.rsplit(':').last().unwrap().to_owned()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("tap-proxy: ! {:?}", args);

    println!("tap-proxy: connecting to {}", args.domain());

    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    let mut id: u64 = 0;
    loop {
        id = id + 1;
        let (client, addr) = listener.accept().await?;
        println!("{}{} {}", "#".red(), id.to_string().red(), addr.to_string().red());
        let cfg = args.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy(id, client, cfg.clone()).await {
                eprintln!("connection error: {}", e);
            }
        });
    }
}

trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {}

impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> AsyncStream for T {}

async fn connect(cfg: Args) -> anyhow::Result<Box<dyn AsyncStream>> {
    let upstream = tokio::net::TcpStream::connect(cfg.dest.clone()).await?;
    if !cfg.tls {
        return anyhow::Ok(Box::new(upstream));
    }
    let mut root_cert_store = tokio_rustls::rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().unwrap() {
        root_cert_store.add(cert)?;
    }

    // 配置 TLS 客户端
    let config = tokio_rustls::rustls::ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth(); // 通常客户端不需要提供证书
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));

    // 将 TcpStream 升级为 TlsStream
    // 注意：需要将域名转换为 rustls::pki_types::ServerName
    let domain = cfg
        .domain() // 替换为 cfg.dest 对应的域名
        .try_into()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid dnsname"))?;

    let tls_stream = connector.connect(domain, upstream).await?;
    Ok(Box::new(tls_stream))
}

const BUFFER_SIZE: usize = 102400; // 100 KB

async fn proxy(id: u64, mut client: tokio::net::TcpStream, cfg: Args) -> anyhow::Result<()> {
    let upstream = connect(cfg.clone()).await?;

    let (mut cr, mut cw) = client.split();
    let (mut sr, mut sw) = tokio::io::split(upstream);
    let client_to_server = async {
        let mut buf = [0u8; BUFFER_SIZE];
        let mut first_packet = true;
        loop {
            let n = cr.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let mut data_to_send: Vec<u8> = buf[..n].to_vec();
            if first_packet {
                first_packet = false;
                data_to_send = fix_header(&buf[..n], cfg.clone());
            }
            print_request(id, &data_to_send);
            sw.write_all(&data_to_send).await?;
        }

        Ok::<_, anyhow::Error>(())
    };

    let server_to_client = async {
        let mut buf = [0u8; BUFFER_SIZE];

        loop {
            let n = sr.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            print_response(id, &buf[..n]);

            cw.write_all(&buf[..n]).await?;
        }

        Ok::<_, anyhow::Error>(())
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

static HTTP_METHODS: &[&str] = &[
    "GET ", "POST ", "PUT ", "DELETE ", "HEAD ", "OPTIONS ", "PATCH ", "CONNECT ", "TRACE ",
];

fn fix_header(bf: &[u8], cfg: Args) -> Vec<u8> {
    let content = String::from_utf8_lossy(bf);
    if !HTTP_METHODS.iter().any(|&m| content.starts_with(m)) {
        return bf.to_vec();
    }

    // 寻找 Header 和 Body 的分界线
    let header_end_idx = content.find("\r\n\r\n");
    if header_end_idx.is_none() {
        return bf.to_vec();
    }
    let (headers, body) = content.split_at(header_end_idx.unwrap());
    let mut lines: Vec<String> = headers.lines().map(|s| s.to_string()).collect();
    let mut host_found = false;

    let domain = cfg.domain();

    // 遍历每一行查找 Host
    for line in lines.iter_mut() {
        if line.to_lowercase().starts_with("host:") {
            *line = format!("Host: {}", domain);
            host_found = true;
            break;
        }
    }

    if cfg.strip_compression {
        for line in lines.iter_mut() {
            if line.to_lowercase().starts_with("accept-encoding:") {
                let msg = format!("[DEBUG] [FixHeader] replace ({} --> identity)", line);
                eprintln!("{}", msg.dimmed());
                *line = format!("Accept-Encoding: {}", "identity"); //不编码
                break;
            }
        }
    }

    // 如果没找到 Host 字段，在第一行（请求行）之后插入
    if !host_found && lines.len() > 0 {
        lines.insert(1, format!("Host: {}", domain));
    }

    // 重新拼接 Request
    let new_header = lines.join("\r\n");
    let mut final_packet = new_header.into_bytes();
    final_packet.extend_from_slice(body.as_bytes()); // 把 \r\n\r\n 和 body 接回去
    final_packet
}

fn print_request(client_id: u64, data: &[u8]) {
    let prefix = format!(
        "{}{} {} ({} bytes)",
        "#".red(),
        client_id.to_string().red(),
        "Request:".blue(),
        data.len()
    );
    println!("{}", prefix);

    print_mixed_data(data);
}

fn print_response(client_id: u64, data: &[u8]) {
    let prefix = format!(
        "{}{} {} ({} bytes)",
        "#".red(),
        client_id.to_string().red(),
        "Response:".cyan(),
        data.len()
    );
    println!("{}", prefix);
    print_mixed_data(data);
}

fn print_mixed_data(data: &[u8]) {
    //  尝试解析 UTF-8
    match std::str::from_utf8(data) {
        Ok(text) => {
            // 全段都是合法的 UTF-8
            println!("{}", text);
        }
        Err(e) => {
            // 发现非法字符，找到合法的截止位置
            let valid_len = e.valid_up_to();
            let (valid_part, binary_part) = data.split_at(valid_len);

            // 打印合法文本部分
            if !valid_part.is_empty() {
                let text = String::from_utf8_lossy(valid_part);
                println!("{}", text.dimmed()); // 用灰色表示这部分已正常解析
            }

            // 打印剩余的二进制/截断部分
            println!("{}", "--- Hex View ---".magenta());
            print_binary(binary_part);
        }
    }
}

const CHUNK_SIZE: usize = 120;
fn print_binary(data: &[u8]) {
    // 使用 chunks 方法按 120 字节切分
    for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
        // 1. 生成 Char 视图 (将不可见字符替换为点，保持位置对应)
        let char_view: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '?' // 非打印字符统一用?，避免干扰终端排版
                }
            })
            .collect();

        // 2. 生成 Hex 视图
        let hex_view = chunk.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>();

        // 3. 打印输出
        // 打印行号偏移量 (Hex 格式)
        let offset = i * CHUNK_SIZE;
        println!(
            "{}",
            format!("Offset <{},{}> ({} bytes):", offset, offset + chunk.len(), chunk.len()).dimmed()
        );

        // 打印字符行 (绿色)
        println!("  {} | {}", "CHR".blue(), char_view.green());

        // 打印 Hex 行 (黄色)
        for (_i, chunk) in hex_view.chunks(CHUNK_SIZE / 3).enumerate() {
            println!("  {} | {}", "HEX".blue(), chunk.join(" ").yellow().dimmed());
        }

        // 行间分割线
        println!("{}", "-".repeat(CHUNK_SIZE + 8).dimmed());
    }
}

// todo: 修改为按照长度截取，每120个字符打印一次，分别打印为 Char 类型和  Hex 类型
// fn print_binary(data: &[u8]){
//     // 按换行符 \n 切分，保留换行符在每一行末尾
//     let lines = data.split_inclusive(|&b| b == b'\n');
//
//     for (idx, line) in lines.enumerate() {
//         // 1. 生成 Lossy 字符串预览
//         // 将不可见字符（除换行外）替换为点，防止终端控制符乱跳
//         let mut text_view = String::new();
//         for &b in line {
//             if b.is_ascii_graphic() || b == b' ' {
//                 text_view.push(b as char);
//             } else if b == b'\n' {
//                 text_view.push_str("\\n");
//             } else if b == b'\r' {
//                 text_view.push_str("\\r");
//             } else {
//                 text_view.push('·'); // 不可打印字符用弱化的点表示
//             }
//         }
//
//         // 2. 生成 Hex 预览
//         let hex_view: String = line.iter()
//             .map(|b| format!("{:02x}", b))
//             .collect::<Vec<String>>()
//             .join(" ");
//
//         // 3. 格式化输出
//         // 行号用灰色，文本部分用绿色，Hex 部分用黄色
//         println!("{:>3} | {:<40}",idx.to_string().dimmed(),text_view.green());
//         println!("{:>3} | {}", idx.to_string().dimmed(),hex_view.yellow().dimmed());
//     }
//     println!("{}", "--- End of Data ---".dimmed());
// }

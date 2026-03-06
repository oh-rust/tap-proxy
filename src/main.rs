use clap::Parser;
use colored::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Args {
    #[arg(short, value_name = "l", default_value = "127.0.0.1:8085", help = "Listen address")]
    pub listen: String,

    #[arg(
        short,
        value_name = "d",
        help = "Destination server address, including hostname and port, e.g. example.com:80"
    )]
    pub dest: String,

    #[arg(
        short,
        long,
        value_name = "s",
        default_value_t = true,
        help = "Use TLS when connecting to the destination server"
    )]
    pub tls: bool,
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

async fn proxy(id: u64, mut client: tokio::net::TcpStream, cfg: Args) -> anyhow::Result<()> {
    let upstream = connect(cfg.clone()).await?;

    let (mut cr, mut cw) = client.split();
    let (mut sr, mut sw) = tokio::io::split(upstream);
    let dynamic_host = cfg.domain();
    let client_to_server = async {
        let mut buf = [0u8; 8192];
        let mut first_packet = true;
        loop {
            let n = cr.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let mut data_to_send: Vec<u8> = buf[..n].to_vec();
            if first_packet {
                first_packet = false;
                data_to_send = fix_header(&buf[..n], dynamic_host.clone());
            }
            print_request(id, &data_to_send);
            sw.write_all(&data_to_send).await?;
        }

        Ok::<_, anyhow::Error>(())
    };

    let server_to_client = async {
        let mut buf = [0u8; 8192];

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

fn fix_header(bf: &[u8], domain: String) -> Vec<u8> {
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

    // 遍历每一行查找 Host
    for line in lines.iter_mut() {
        if line.to_lowercase().starts_with("host:") {
            *line = format!("Host: {}", domain);
            host_found = true;
            break;
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
    let prefix = format!("{}{} {}", "#".red(), client_id.to_string().red(), "Request:".blue());
    println!("{}", prefix);

    if let Ok(text) = std::str::from_utf8(data) {
        println!("{}", text);
    } else {
        println!("<{} bytes binary>", data.len());
    }
}

fn print_response(client_id: u64, data: &[u8]) {
    let prefix = format!("{}{} {}", "#".red(), client_id.to_string().red(), "Response:".cyan());
    println!("{}", prefix);
    if let Ok(text) = std::str::from_utf8(data) {
        println!("{}", text);
    } else {
        println!("<{} bytes binary>", data.len());
    }
}

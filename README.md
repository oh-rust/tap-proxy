# tap-proxy

`tap-proxy` 是一个使用 rust 开发的轻量级的 **TCP ( 包含 HTTP / HTTPS ) 调试代理工具**。  
它可以将本地端口收到的 TCP 请求转发到指定的目标服务器，并在终端实时打印 **Request** 和 **Response** 内容，方便在开发过程中调试 API 调用。

适用于：
- 调试后端 API，观察第三方接口请求/响应
- 本地开发时的简单转发代理
- 快速排查 TCP 请求问题


当你的程序调用远程 API 时，可以把 API 地址指向 tap-proxy：
```
你的程序  ------>  tap-proxy  ------> 目标服务器
```
这样就可以在 `tap-proxy` 看到：
- TCP Request
- TCP Response

HTTP 状态码
---

# 功能特点

- 本地端口监听并转发 TCP 请求（包括  **HTTP** 和 **HTTPS**）
- 实时打印 **Request / Response**
- 支持 TLS 协议卸载
- 无需复杂配置，单命令启动
- 支持 HTTP 协议禁用压缩（声明禁用 gzip 等压缩，让 HTTP Server 不压缩响应），以方便观察

---

# 安装

```bash
cargo install --git https://github.com/oh-rust/tap-proxy --branch master
```

# 使用方法
## 1. 参数说明
```
Usage: tap-proxy [OPTIONS] -d <d>

Options:
  -l <l>         Listen address [default: 127.0.0.1:8085]
  -d <d>         Destination server address, including hostname and port, e.g. example.com:80
  -t, --tls      Use TLS when connecting to the destination server
  -z, --strip-compression  Strip 'Accept-Encoding' headers to prevent compressed responses
  -h, --help     Print help
  -V, --version  Print version
```

`-t` 参数用于 `tls` 协议卸载，如此可实现在终端打印出明文信息：
```
你的程序 ---（明文）---> tap-proxy ---（加密）---> 目标服务器
```

使用 `-z`参数， 对于 HTTP/HTTPS 请求，会修改 `Header` 中的 `Accept-Encoding` 字段，
让 Server 返回明文内容（非压缩），以方便在 `tap-proxy` 观察到明文的 `HTTP Response`。

## 2. HTTP 转发
启动代理：
```bash
tap-proxy -l 127.0.0.1:8090 -d example.com:8080
```

例如发送请求： `http://127.0.0.1:8090/api/some`  
tap-proxy 会自动转发为：`http://example.com:8080/api/some`

## 3. HTTPS 转发

启动代理：
```bash
tap-proxy -l 127.0.0.1:8090 -d example.com:443 -t
```

发送请求：`http://127.0.0.1:8090/api/some`  
tap-proxy 会自动转发为：`https://example.com:443/api/some`
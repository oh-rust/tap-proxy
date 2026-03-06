# tap-proxy

`tap-proxy` 是一个使用 rust 开发的轻量级的 **TCP ( HTTP / HTTPS ) 调试代理工具**。  
它可以将本地端口收到的 TCP 请求转发到指定的目标服务器，并在终端实时打印 **Request** 和 **Response** 内容，方便在开发过程中调试 API 调用。

适用于：
- 调试后端 API
- 观察第三方接口请求/响应
- 本地开发时的简单转发代理
- 快速排查 TCP 请求问题


当你的程序调用远程 API 时，可以把 API 地址指向 tap-proxy：
```
你的程序 -> tap-proxy -> 真实 API
```
这样就可以看到：
- 请求头
- 请求体
- 返回数据

HTTP 状态码
---

# 功能特点

- 本地端口监听并转发 TCP (包含 HTTP) 请求
- 支持转发到 **HTTP** 或 **HTTPS**
- 实时打印 **Request / Response**
- 无需复杂配置，单命令启动
- 轻量级 CLI 工具

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
  -h, --help     Print help
  -V, --version  Print version
```

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
tap-proxy -l 127.0.0.1:8090 -d example.com:443 -s
```

发送请求：`http://127.0.0.1:8090/api/some`  
tap-proxy 会自动转发为：`https://example.com:443/api/some`
//! Minimal RTSP/1.0 + HTTP/1.1 request reader and response writer (blocking).
use std::io::{self, BufRead, Read, Write};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
/// Cover art can be ~1 MB; anything bigger is not a legitimate AirPlay request.
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub protocol: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn content_type(&self) -> &str {
        self.header("Content-Type").unwrap_or("")
    }
    pub fn is_rtsp(&self) -> bool {
        self.protocol.starts_with("RTSP/")
    }
}

/// Reads one request. `Ok(None)` = clean EOF between requests.
pub fn read_request<R: BufRead>(r: &mut R) -> io::Result<Option<Request>> {
    let mut line = String::new();
    let mut total = 0usize;
    // skip stray empty lines (some senders send CRLF keep-alives)
    loop {
        line.clear();
        let n = read_line_limited(r, &mut line, MAX_HEADER_BYTES)?;
        if n == 0 {
            return Ok(None);
        }
        total += n;
        if !line.trim().is_empty() {
            break;
        }
        if total > MAX_HEADER_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too many blank lines"));
        }
    }
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or("").to_string();
    let url = parts.next().unwrap_or("").to_string();
    let protocol = parts.next().unwrap_or("").to_string();
    if method.is_empty() || url.is_empty() || !(protocol.starts_with("RTSP/") || protocol.starts_with("HTTP/")) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("bad request line {line:?}")));
    }
    let mut req = Request { method, url, protocol, ..Default::default() };
    loop {
        line.clear();
        let n = read_line_limited(r, &mut line, MAX_HEADER_BYTES)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        total += n;
        if total > MAX_HEADER_BYTES || req.headers.len() > MAX_HEADERS {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            req.headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = req.header("Content-Length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body)?;
    Ok(Some(req))
}

fn read_line_limited<R: BufRead>(r: &mut R, out: &mut String, limit: usize) -> io::Result<usize> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(limit as u64 + 1).read_until(b'\n', &mut buf)?;
    if n > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
    }
    out.push_str(&String::from_utf8_lossy(&buf));
    Ok(n)
}

pub struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Close the connection after sending.
    pub close: bool,
}

impl Response {
    pub fn ok() -> Self {
        Response { status: 200, reason: "OK", headers: Vec::new(), body: Vec::new(), close: false }
    }
    pub fn status(status: u16, reason: &'static str) -> Self {
        Response { status, reason, ..Response::ok() }
    }
    pub fn header(mut self, k: &str, v: impl Into<String>) -> Self {
        self.headers.push((k.to_string(), v.into()));
        self
    }
    pub fn with_body(mut self, content_type: &str, body: Vec<u8>) -> Self {
        self.headers
            .push(("Content-Type".to_string(), content_type.to_string()));
        self.body = body;
        self
    }
    pub fn plist(self, v: &plist::Value) -> Self {
        let mut out = Vec::new();
        if let Err(e) = v.to_writer_binary(&mut out) {
            log::error!("plist encode: {e}");
        }
        self.with_body("application/x-apple-binary-plist", out)
    }

    pub fn write_to<W: Write>(&self, protocol: &str, w: &mut W) -> io::Result<()> {
        let mut head = format!("{} {} {}\r\n", protocol, self.status, self.reason);
        for (k, v) in &self.headers {
            head.push_str(k);
            head.push_str(": ");
            head.push_str(v);
            head.push_str("\r\n");
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        let mut out = head.into_bytes();
        out.extend_from_slice(&self.body);
        w.write_all(&out)?;
        w.flush()
    }
}

pub fn write_response<W: Write>(w: &mut W, req: &Request, resp: &Response) -> io::Result<()> {
    let mut head = format!("{} {} {}\r\n", req.protocol, resp.status, resp.reason);
    if let Some(cseq) = req.header("CSeq") {
        head.push_str(&format!("CSeq: {cseq}\r\n"));
    }
    head.push_str("Server: AirTunes/220.68\r\n");
    for (k, v) in &resp.headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", resp.body.len()));
    let mut out = head.into_bytes();
    out.extend_from_slice(&resp.body);
    w.write_all(&out)?;
    w.flush()
}

#[cfg(test)]
pub fn read_request_or_response_for_test<R: BufRead>(r: &mut R) -> Vec<u8> {
    let mut line = String::new();
    let mut content_len = 0usize;
    loop {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let t = line.trim();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            if k.trim().eq_ignore_ascii_case("Content-Length") {
                content_len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; content_len];
    r.read_exact(&mut body).unwrap();
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_rtsp() {
        let raw = b"POST /pair-setup RTSP/1.0\r\nCSeq: 3\r\nContent-Length: 4\r\nX-Apple-HKP: 3\r\n\r\nabcdGET /info RTSP/1.0\r\nCSeq: 4\r\n\r\n";
        let mut r = io::BufReader::new(&raw[..]);
        let a = read_request(&mut r).unwrap().unwrap();
        assert_eq!(a.method, "POST");
        assert_eq!(a.url, "/pair-setup");
        assert_eq!(a.header("cseq"), Some("3"));
        assert_eq!(a.body, b"abcd");
        let b = read_request(&mut r).unwrap().unwrap();
        assert_eq!(b.url, "/info");
        assert!(read_request(&mut r).unwrap().is_none());
    }
}

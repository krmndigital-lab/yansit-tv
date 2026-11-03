//! Minimal DMAP (DAAP) TLV parser for `SET_PARAMETER` `application/x-dmap-tagged`.

use super::Metadata;

pub fn parse_metadata(data: &[u8]) -> Metadata {
    let mut m = Metadata::default();
    walk(data, &mut m);
    m
}

pub fn parse(data: &[u8]) -> Metadata {
    parse_metadata(data)
}

fn walk(mut buf: &[u8], out: &mut Metadata) {
    while buf.len() >= 8 {
        let tag = &buf[..4];
        let len = u32::from_be_bytes(buf[4..8].try_into().unwrap()) as usize;
        buf = &buf[8..];
        if len > buf.len() {
            break;
        }
        let val = &buf[..len];
        buf = &buf[len..];
        match tag {
            b"mlit" | b"mstt" | b"agar" => walk(val, out),
            b"minm" => out.title = String::from_utf8_lossy(val).into_owned(),
            b"asar" => out.artist = String::from_utf8_lossy(val).into_owned(),
            b"asal" => out.album = String::from_utf8_lossy(val).into_owned(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dmap_nested_mlit() {
        let mut inner = Vec::new();
        inner.extend_from_slice(b"minm\x00\x00\x00\x05Deniz");
        inner.extend_from_slice(b"asar\x00\x00\x00\x06Mor ve");
        inner.extend_from_slice(b"asal\x00\x00\x00\x05Dunya");
        let mut outer = Vec::from(&b"mlit"[..]);
        outer.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        outer.extend_from_slice(&inner);
        let m = parse_metadata(&outer);
        assert_eq!(m.title, "Deniz");
        assert_eq!(m.artist, "Mor ve");
        assert_eq!(m.album, "Dunya");
    }
}

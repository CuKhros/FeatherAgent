//! 长度前缀帧（§4.1 / §7.2）。
//!
//! 编码可协商：v1 默认 stdio JSON-RPC 兼容用 `Codec::Json`，
//! v1.1 起默认 `Codec::MsgPack`，两者帧头一致（`u32 LE` 长度 + 体），
//! 因此换编码层不影响契约方法集。

use std::io::{self, Read, Write};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    MsgPack,
    Json,
}

impl Codec {
    pub fn parse(s: &str) -> Option<Codec> {
        match s {
            "msgpack" | "mp" => Some(Codec::MsgPack),
            "json" => Some(Codec::Json),
            _ => None,
        }
    }
}

pub fn write_frame<W: Write>(w: &mut W, v: &Value, codec: Codec) -> io::Result<()> {
    let body: Vec<u8> = match codec {
        Codec::MsgPack => rmp_serde::to_vec_named(v)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?,
        Codec::Json => serde_json::to_vec(v)?,
    };
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

/// 读到干净 EOF 返回 `Ok(None)`。
pub fn read_frame<R: Read>(r: &mut R, codec: Codec) -> io::Result<Option<Value>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let v = match codec {
        Codec::MsgPack => rmp_serde::from_slice(&body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?,
        Codec::Json => serde_json::from_slice(&body)?,
    };
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_frame_is_u32_le_prefix_plus_utf8() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"jsonrpc":"2.0","id":1}), Codec::Json).unwrap();
        assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_le_bytes());
        let body = std::str::from_utf8(&buf[4..]).unwrap();
        assert!(body.contains("\"jsonrpc\":\"2.0\""));
        let mut r = &buf[..];
        let v = read_frame(&mut r, Codec::Json).unwrap().unwrap();
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn msgpack_frame_is_named_map_and_roundtrips() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"a":1,"b":"x"}), Codec::MsgPack).unwrap();
        assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_le_bytes());
        assert_eq!(buf[4], 0x82, "fixmap with two named fields");
        let mut r = &buf[..];
        let v = read_frame(&mut r, Codec::MsgPack).unwrap().unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], "x");
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut hdr = (64u32 * 1024 * 1024 + 1).to_le_bytes().to_vec();
        hdr.extend_from_slice(&[]);
        let err = read_frame(&mut &hdr[..], Codec::Json).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}

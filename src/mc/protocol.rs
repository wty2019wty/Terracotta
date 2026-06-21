//! Minecraft Server List Ping protocol implementation.
//!
//! Supports both the modern Java Edition protocol (1.7+) and the legacy
//! protocol (pre-1.7), used to check whether a Minecraft server is alive
//! on a given localhost port.

use socket2::{Domain, SockAddr, Socket, Type};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

const PING_TIMEOUT: Duration = Duration::from_secs(5);

// ── Varint utilities ──────────────────────────────────────────────────────────

/// Encodes a signed 32-bit integer as a Minecraft varint.
fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut remaining = value as u32;
    loop {
        if remaining & !0x7F == 0 {
            buf.push(remaining as u8);
            break;
        }
        buf.push(((remaining & 0x7F) | 0x80) as u8);
        remaining >>= 7;
    }
}

/// Decodes a varint from a byte slice. Returns the value and bytes consumed.
fn read_varint_slice(data: &[u8]) -> Option<(i32, usize)> {
    let mut result: i32 = 0;
    for i in 0..5 {
        if i >= data.len() {
            return None;
        }
        let byte = data[i];
        result |= ((byte & 0x7F) as i32) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
    }
    None
}

/// Decodes a varint from a reader (e.g. a socket).
fn read_varint(reader: &mut impl Read) -> Option<i32> {
    let mut result: i32 = 0;
    for i in 0..5 {
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf).ok()?;
        let byte = buf[0];
        result |= ((byte & 0x7F) as i32) << (7 * i);
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}

/// Writes a Minecraft UTF-8 string (varint-length-prefixed) into the buffer.
fn write_mc_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

// ── Connection helper ─────────────────────────────────────────────────────────

/// Opens a TCP connection to localhost:port with a timeout.
fn connect(port: u16) -> Option<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).ok()?;
    socket.set_read_timeout(Some(PING_TIMEOUT)).ok()?;
    socket.set_write_timeout(Some(PING_TIMEOUT)).ok()?;
    let addr = SockAddr::from(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));
    socket.connect_timeout(&addr, PING_TIMEOUT).ok()?;
    Some(socket)
}

// ── Modern Java Edition protocol (1.7+) ───────────────────────────────────────

/// Attempts to ping a Minecraft server using the modern 1.7+ Server List Ping
/// protocol (handshake + status request). Returns true if the server responds
/// with a valid JSON status payload.
pub fn ping_modern(port: u16) -> bool {
    let mut socket = match connect(port) {
        Some(s) => s,
        None => return false,
    };

    // 1. Build handshake packet (packet ID 0x00)
    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00);       // packet ID: handshake
    write_varint(&mut handshake, -1);          // protocol version (-1 = handshake-only)
    write_mc_string(&mut handshake, "localhost");
    handshake.extend_from_slice(&port.to_be_bytes()); // port as unsigned short (big-endian)
    write_varint(&mut handshake, 1);           // next state: 1 = status

    let mut framed_handshake = Vec::new();
    write_varint(&mut framed_handshake, handshake.len() as i32);
    framed_handshake.extend_from_slice(&handshake);

    // 2. Build status request packet (packet ID 0x00)
    let mut status_request = Vec::new();
    write_varint(&mut status_request, 0x00);  // packet ID: status request

    let mut framed_request = Vec::new();
    write_varint(&mut framed_request, status_request.len() as i32);
    framed_request.extend_from_slice(&status_request);

    // 3. Send handshake + status request
    if socket.write_all(&framed_handshake).is_err() {
        return false;
    }
    if socket.write_all(&framed_request).is_err() {
        return false;
    }

    // 4. Read response: outer varint = packet length
    let packet_len = match read_varint(&mut socket) {
        Some(len) if len > 0 => len as usize,
        _ => return false,
    };

    // Read the packet body
    let mut packet_data = vec![0u8; packet_len];
    if socket.read_exact(&mut packet_data).is_err() {
        return false;
    }

    // 5. Parse response: packet ID (varint) should be 0x00
    let (packet_id, offset) = match read_varint_slice(&packet_data) {
        Some(v) => v,
        None => return false,
    };
    if packet_id != 0 {
        return false;
    }

    // 6. Read JSON string length (varint)
    let (json_len, json_offset) = match read_varint_slice(&packet_data[offset..]) {
        Some(v) => v,
        None => return false,
    };
    if json_len <= 0 {
        return false;
    }

    // 7. Extract and validate JSON
    let json_start = offset + json_offset;
    let json_end = json_start + json_len as usize;
    if json_end > packet_data.len() {
        return false;
    }

    serde_json::from_slice::<serde_json::Value>(&packet_data[json_start..json_end]).is_ok()
}

// ── Legacy protocol (pre-1.7) ─────────────────────────────────────────────────

/// The magic bytes for the legacy server list ping request.
/// See: https://minecraft.wiki/w/Java_Edition_protocol/Server_List_Ping
const LEGACY_REQUEST: [u8; 3] = [0xFE, 0x01, 0xFA];

/// Attempts to ping a Minecraft server using the pre-1.7 legacy Server List Ping
/// protocol. Returns true if the server responds with a valid kick packet.
pub fn ping_legacy(port: u16) -> bool {
    let mut socket = match connect(port) {
        Some(s) => s,
        None => return false,
    };

    // 1. Send the magic bytes
    if socket.write_all(&LEGACY_REQUEST).is_err() {
        return false;
    }

    // 2. Read packet ID (should be 0xFF)
    let mut packet_id_buf = [0u8; 1];
    if socket.read_exact(&mut packet_id_buf).is_err() {
        return false;
    }
    if packet_id_buf[0] != 0xFF {
        return false;
    }

    // 3. Read string length (unsigned short, big-endian, in UTF-16 characters)
    let mut len_buf = [0u8; 2];
    if socket.read_exact(&mut len_buf).is_err() {
        return false;
    }
    let char_len = u16::from_be_bytes(len_buf) as usize;

    // Sanity check on length
    if char_len == 0 || char_len > 65535 {
        return false;
    }

    // 4. Read the string data (UTF-16BE, 2 bytes per character)
    let byte_len = char_len * 2;
    let mut string_data = vec![0u8; byte_len];
    if socket.read_exact(&mut string_data).is_err() {
        return false;
    }

    // 5. Decode and validate the response
    validate_legacy_response(&string_data)
}

/// Validates a legacy protocol response string (UTF-16BE encoded).
/// The response format is one of:
///   - 1.4+:   §1\0<protocol>\0<version>\0<motd>\0<online>\0<max>
///   - pre-1.4: <motd>§<online>§<max>
fn validate_legacy_response(data: &[u8]) -> bool {
    let decoded = decode_utf16be(data);
    let parts: Vec<&str> = decoded.split('\0').collect();

    if parts.is_empty() {
        return false;
    }

    if parts[0] == "§1" {
        // 1.4+ format: §1\0<protocol>\0<version>\0<motd>\0<online>\0<max>
        if parts.len() < 6 {
            return false;
        }
        // Validate protocol is a parseable number
        parts[1].parse::<u32>().is_ok()
    } else {
        // Pre-1.4 format: <motd>§<online>§<max>
        let legacy_parts: Vec<&str> = parts[0].split('§').collect();
        if legacy_parts.len() < 3 {
            return false;
        }
        // Validate online and max are parseable numbers
        legacy_parts[1].parse::<u32>().is_ok() && legacy_parts[2].parse::<u32>().is_ok()
    }
}

/// Decodes UTF-16BE bytes into a Rust String.
fn decode_utf16be(data: &[u8]) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let code_unit = u16::from_be_bytes([data[i], data[i + 1]]);
        if let Some(ch) = char::from_u32(code_unit as u32) {
            result.push(ch);
        }
        i += 2;
    }
    result
}

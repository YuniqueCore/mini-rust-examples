use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::query::{QTYPE_A, QTYPE_AAAA};

pub fn parse(
    pkt: &[u8],
    expect_id: u16,
    qtype: u16,
) -> Result<(Vec<IpAddr>, u32, bool /*tc*/), ()> {
    if pkt.len() < 12 {
        return Err(());
    }

    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    if id != expect_id {
        return Err(());
    }

    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    let tc = (flags & 0x0200) != 0;
    let rcode = (flags & 0x000F) as u8;

    let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;

    // NXDOMAIN 或其它错误
    if rcode != 0 {
        return Ok((vec![], 60, tc)); // 负缓存 TTL 给个保守值（可优化）
    }

    let mut off = 12usize;

    // skip questions
    for _ in 0..qd {
        let _ = read_name(pkt, &mut off)?;
        let _ = read_u16(pkt, &mut off)?;
        let _ = read_u16(pkt, &mut off)?;
    }

    let mut ips = Vec::new();
    let mut min_ttl: Option<u32> = None;

    for _ in 0..an {
        let _name = read_name(pkt, &mut off)?; // 不用也要读以推进 offset
        let typ = read_u16(pkt, &mut off)?;
        let _class = read_u16(pkt, &mut off)?;
        let ttl = read_u32(pkt, &mut off)?;
        let rdlen = read_u16(pkt, &mut off)? as usize;

        if off + rdlen > pkt.len() {
            return Err(());
        }

        if typ == qtype {
            match typ {
                QTYPE_A if rdlen == 4 => {
                    let ip = Ipv4Addr::new(pkt[off], pkt[off + 1], pkt[off + 2], pkt[off + 3]);
                    ips.push(IpAddr::V4(ip));
                }
                QTYPE_AAAA if rdlen == 16 => {
                    let mut b = [0u8; 16];
                    b.copy_from_slice(&pkt[off..off + 16]);
                    ips.push(IpAddr::V6(Ipv6Addr::from(b)));
                }
                _ => {}
            }
            min_ttl = Some(min_ttl.map(|x| x.min(ttl)).unwrap_or(ttl));
        }

        off += rdlen;
    }

    Ok((ips, min_ttl.unwrap_or(60), tc))
}

fn read_u16(pkt: &[u8], off: &mut usize) -> Result<u16, ()> {
    if *off + 2 > pkt.len() {
        return Err(());
    }
    let v = u16::from_be_bytes([pkt[*off], pkt[*off + 1]]);
    *off += 2;
    Ok(v)
}

fn read_u32(pkt: &[u8], off: &mut usize) -> Result<u32, ()> {
    if *off + 4 > pkt.len() {
        return Err(());
    }
    let v = u32::from_be_bytes([pkt[*off], pkt[*off + 1], pkt[*off + 2], pkt[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_name(pkt: &[u8], off: &mut usize) -> Result<String, ()> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut cur = *off;
    let mut guard = 0;

    loop {
        guard += 1;
        if guard > 128 {
            return Err(());
        } // 防止恶意循环指针

        if cur >= pkt.len() {
            return Err(());
        }
        let len = pkt[cur];

        // pointer: 11xxxxxx xxxxxxxx
        if (len & 0xC0) == 0xC0 {
            if cur + 1 >= pkt.len() {
                return Err(());
            }
            let b2 = pkt[cur + 1];
            let ptr = (((len as u16 & 0x3F) << 8) | b2 as u16) as usize;

            if !jumped {
                *off = cur + 2; // 原始偏移只前进 2
                jumped = true;
            }
            cur = ptr;
            continue;
        }

        // end
        if len == 0 {
            if !jumped {
                *off = cur + 1;
            }
            break;
        }

        let l = len as usize;
        cur += 1;
        if cur + l > pkt.len() {
            return Err(());
        }
        let label = std::str::from_utf8(&pkt[cur..cur + l]).map_err(|_| ())?;
        labels.push(label.to_string());
        cur += l;
    }

    Ok(labels.join("."))
}

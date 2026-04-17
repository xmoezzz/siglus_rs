use crate::bitpack::{PackBuf, LOTS_OF_BITS, PB_WINDOW_SIZE};
use crate::codec::{TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES};
use crate::error::{Result, TheoraError};
use crate::huffman::OC_NDCT_TOKEN_BITS;

const OC_DCT_TOKEN_MAP: [u8; TH_NDCT_TOKENS] = [
    15, 16, 17, 88, 80, 1, 0, 48, 14, 56, 57, 58, 59, 60, 62, 64, 66, 68, 72, 2, 4, 6, 8, 18, 20,
    22, 24, 26, 32, 12, 28, 40,
];

const OC_DCT_TOKEN_MAP_LOG_NENTRIES: [u8; TH_NDCT_TOKENS] = [
    0, 0, 0, 2, 3, 0, 0, 3, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 3, 1, 1, 1, 2, 1, 1, 1, 1, 1, 3, 1, 2, 3,
];

const HUFF_SLUSH: i32 = 2;
const ROOT_HUFF_SLUSH: i32 = 7;

pub fn huff_tree_unpack(opb: &mut PackBuf<'_>, tokens: &mut [[u8; 2]; 256]) -> Result<usize> {
    let mut code: u32 = 0;
    let mut len: i32 = 0;
    let mut ntokens: usize = 0;
    let mut nleaves: i32 = 0;
    loop {
        let bits = opb.read1();
        if opb.bytes_left() < 0 {
            return Err(TheoraError::BadHeader);
        }
        if bits == 0 {
            len += 1;
            if len > 32 {
                return Err(TheoraError::BadHeader);
            }
        } else {
            nleaves += 1;
            if nleaves > 32 {
                return Err(TheoraError::BadHeader);
            }
            let bits = opb.read(OC_NDCT_TOKEN_BITS) as usize;
            let neb = OC_DCT_TOKEN_MAP_LOG_NENTRIES[bits] as usize;
            let mut token = OC_DCT_TOKEN_MAP[bits];
            let mut nentries = 1usize << neb;
            while nentries > 0 {
                tokens[ntokens][0] = token;
                tokens[ntokens][1] = (len as usize + neb) as u8;
                ntokens += 1;
                token = token.wrapping_add(1);
                nentries -= 1;
            }
            if len <= 0 {
                break;
            }
            let mut code_bit = 0x8000_0000u32 >> ((len - 1) as u32);
            while len > 0 && (code & code_bit) != 0 {
                code ^= code_bit;
                code_bit = code_bit.wrapping_shl(1);
                len -= 1;
            }
            if len <= 0 {
                break;
            }
            code |= code_bit;
        }
    }
    Ok(ntokens)
}

fn huff_subtree_tokens(tokens: &[[u8; 2]], depth: i32) -> usize {
    let mut code: u32 = 0;
    let mut ti: usize = 0;
    loop {
        let d = i32::from(tokens[ti][1]) - depth;
        if d < 32 {
            code = code.wrapping_add(0x8000_0000u32 >> (d as u32));
            ti += 1;
        } else {
            code = code.wrapping_add(1);
            ti += huff_subtree_tokens(&tokens[ti..], depth + 31);
        }
        if code >= 0x8000_0000u32 {
            break;
        }
    }
    ti
}

fn huff_tree_collapse_depth(tokens: &[[u8; 2]], ntokens: usize, depth: i32) -> i32 {
    let slush = if depth > 0 {
        HUFF_SLUSH
    } else {
        ROOT_HUFF_SLUSH
    };
    let mut nbits = 1;
    let mut occupancy = 2;
    let mut got_leaves = true;
    let mut best_nbits = 1;
    loop {
        if got_leaves {
            best_nbits = nbits;
        }
        nbits += 1;
        got_leaves = false;
        let loccupancy = occupancy;
        occupancy = 0;
        let mut ti = 0usize;
        while ti < ntokens {
            occupancy += 1;
            let tdepth = i32::from(tokens[ti][1]);
            if tdepth < depth + nbits {
                ti += 1;
            } else if tdepth == depth + nbits {
                got_leaves = true;
                ti += 1;
            } else {
                ti += huff_subtree_tokens(&tokens[ti..], depth + nbits);
            }
        }
        if !(occupancy > loccupancy && occupancy * slush >= (1 << nbits)) {
            break;
        }
    }
    best_nbits
}

fn huff_node_size(nbits: i32) -> usize {
    1usize + (1usize << (nbits as usize))
}

fn huff_tree_collapse_into(tree: Option<&mut [i16]>, tokens: &[[u8; 2]], ntokens: usize) -> usize {
    let mut node = [0i16; 34];
    let mut depth = [0u8; 34];
    let mut last = [0u8; 34];
    depth[0] = 0;
    last[0] = (ntokens - 1) as u8;
    let mut ntree: usize = 0;
    let mut ti: usize = 0;
    let mut level: usize = 0;
    let mut tree = tree;
    loop {
        let mut nbits = huff_tree_collapse_depth(
            &tokens[ti..],
            (last[level] as usize + 1) - ti,
            depth[level] as i32,
        );
        node[level] = ntree as i16;
        ntree += huff_node_size(nbits);
        if let Some(t) = tree.as_deref_mut() {
            t[node[level] as usize] = nbits as i16;
        }
        node[level] += 1;
        loop {
            while ti <= last[level] as usize
                && i32::from(tokens[ti][1]) <= i32::from(depth[level]) + nbits
            {
                if let Some(t) = tree.as_deref_mut() {
                    let shift =
                        (i32::from(depth[level]) + nbits - i32::from(tokens[ti][1])) as usize;
                    let mut nentries = 1usize << shift;
                    let leaf = -((((i32::from(tokens[ti][1]) - i32::from(depth[level])) << 8)
                        | i32::from(tokens[ti][0])) as i16);
                    while nentries > 0 {
                        t[node[level] as usize] = leaf;
                        node[level] += 1;
                        nentries -= 1;
                    }
                }
                ti += 1;
            }
            if ti <= last[level] as usize {
                depth[level + 1] = (i32::from(depth[level]) + nbits) as u8;
                if let Some(t) = tree.as_deref_mut() {
                    t[node[level] as usize] = ntree as i16;
                }
                node[level] += 1;
                level += 1;
                last[level] =
                    (ti + huff_subtree_tokens(&tokens[ti..], depth[level] as i32) - 1) as u8;
                break;
            } else if level == 0 {
                return ntree;
            } else {
                let child_level = level;
                level -= 1;
                nbits = i32::from(depth[child_level]) - i32::from(depth[level]);
            }
        }
    }
}

fn huff_tree_size(tree: &[i16], node: usize) -> usize {
    let n = tree[node] as usize;
    let mut size = huff_node_size(n as i32);
    let nchildren = 1usize << n;
    let mut i = 0usize;
    while i < nchildren {
        let child = tree[node + i + 1];
        if child <= 0 {
            let depth = ((-child) as usize) >> 8;
            i += 1usize << (n - depth);
        } else {
            size += huff_tree_size(tree, child as usize);
            i += 1;
        }
    }
    size
}

pub fn huff_trees_unpack(opb: &mut PackBuf<'_>) -> Result<Vec<Vec<i16>>> {
    let mut out = Vec::with_capacity(TH_NHUFFMAN_TABLES);
    for _ in 0..TH_NHUFFMAN_TABLES {
        let mut tokens = [[0u8; 2]; 256];
        let ntokens = huff_tree_unpack(opb, &mut tokens)?;
        let size = huff_tree_collapse_into(None, &tokens[..ntokens], ntokens);
        if size > 32767 {
            return Err(TheoraError::NotImplemented);
        }
        let mut tree = vec![0i16; size];
        let actual = huff_tree_collapse_into(Some(&mut tree), &tokens[..ntokens], ntokens);
        debug_assert_eq!(size, actual);
        out.push(tree);
    }
    Ok(out)
}

pub fn huff_trees_copy(src: &[Vec<i16>]) -> Result<Vec<Vec<i16>>> {
    if src.len() != TH_NHUFFMAN_TABLES {
        return Err(TheoraError::InvalidArgument);
    }
    let mut out = Vec::with_capacity(src.len());
    for tree in src {
        let size = huff_tree_size(tree, 0);
        let mut copy = vec![0i16; size];
        copy.copy_from_slice(&tree[..size]);
        out.push(copy);
    }
    Ok(out)
}

pub fn huff_token_decode_c(opb: &mut PackBuf<'_>, tree: &[i16]) -> u8 {
    let mut ptr = opb.ptr;
    let stop = opb.data.len();
    let mut window = opb.window;
    let mut available = opb.bits;
    let mut node: i32 = 0;
    loop {
        let n = tree[node as usize] as i32;
        if n > available {
            let mut shift = PB_WINDOW_SIZE - available;
            loop {
                if ptr >= stop {
                    shift = -LOTS_OF_BITS;
                    break;
                }
                shift -= 8;
                window |= (opb.data[ptr] as u64) << (shift as u32);
                ptr += 1;
                if shift < 8 {
                    break;
                }
            }
            available = PB_WINDOW_SIZE - shift;
        }
        let bits = (window >> ((PB_WINDOW_SIZE - n) as u32)) as usize;
        node = i32::from(tree[node as usize + 1 + bits]);
        if node <= 0 {
            let leaf = -node;
            let used = leaf >> 8;
            window <<= used as u32;
            available -= used;
            opb.ptr = ptr;
            opb.window = window;
            opb.bits = available;
            return (leaf & 255) as u8;
        }
        window <<= n as u32;
        available -= n;
    }
}

#[cfg(test)]
mod tests {
    use super::huff_token_decode_c;
    use crate::bitpack::PackBuf;

    #[test]
    fn decode_from_simple_collapsed_tree() {
        let tree = vec![1i16, -261i16, -265i16];
        let data = [0b1000_0000u8];
        let mut pb = PackBuf::new(&data);
        assert_eq!(huff_token_decode_c(&mut pb, &tree), 9);
    }
}

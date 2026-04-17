use crate::codec::{Comment, Info};

fn tag_compare(s1: &[u8], s2: &[u8], n: usize) -> bool {
    if s1.len() < n || s2.len() < n {
        return true;
    }
    for i in 0..n {
        if s1[i].to_ascii_uppercase() != s2[i].to_ascii_uppercase() {
            return true;
        }
    }
    s1.get(n).copied() != Some(b'=')
}

pub fn th_info_init(info: &mut Info) {
    *info = Info::default();
}

pub fn th_info_clear(info: &mut Info) {
    info.clear();
}

pub fn th_comment_init(tc: &mut Comment) {
    *tc = Comment::default();
}

pub fn th_comment_add(tc: &mut Comment, comment: &[u8]) {
    tc.user_comments.push(comment.to_vec());
}

pub fn th_comment_add_tag(tc: &mut Comment, tag: &str, val: &str) {
    let mut comment = Vec::with_capacity(tag.len() + val.len() + 1);
    comment.extend_from_slice(tag.as_bytes());
    comment.push(b'=');
    comment.extend_from_slice(val.as_bytes());
    th_comment_add(tc, &comment);
}

pub fn th_comment_query<'a>(tc: &'a Comment, tag: &str, count: i32) -> Option<&'a [u8]> {
    if count < 0 {
        return None;
    }
    let tag_bytes = tag.as_bytes();
    let mut found = 0i32;
    for comment in &tc.user_comments {
        if !tag_compare(comment, tag_bytes, tag_bytes.len()) {
            if found == count {
                return Some(&comment[tag_bytes.len() + 1..]);
            }
            found += 1;
        }
    }
    None
}

pub fn th_comment_query_count(tc: &Comment, tag: &str) -> i32 {
    let tag_bytes = tag.as_bytes();
    let mut count = 0;
    for comment in &tc.user_comments {
        if !tag_compare(comment, tag_bytes, tag_bytes.len()) {
            count += 1;
        }
    }
    count
}

pub fn th_comment_clear(tc: &mut Comment) {
    tc.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Comment, Info};

    #[test]
    fn info_init_sets_version_defaults() {
        let mut info = Info::zeroed();
        th_info_init(&mut info);
        assert_eq!(info.version_major, 3);
        assert_eq!(info.version_minor, 2);
        assert_eq!(info.version_subminor, 1);
        assert_eq!(info.keyframe_granule_shift, 6);
    }

    #[test]
    fn comment_query_is_case_insensitive() {
        let mut comment = Comment::default();
        th_comment_add_tag(&mut comment, "ARTIST", "Xiph");
        th_comment_add_tag(&mut comment, "artist", "Org");
        assert_eq!(th_comment_query_count(&comment, "ArTiSt"), 2);
        assert_eq!(th_comment_query(&comment, "artist", 1), Some(&b"Org"[..]));
    }
}

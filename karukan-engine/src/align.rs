//! Align a converted surface text with its original hiragana reading.
//!
//! Used to support partial re-conversion from an already-converted state.
//! Given the converted output (mixed kanji/kana) and the original hiragana
//! reading, this produces a segment list that lets the caller map any
//! character range in the surface back to the corresponding reading range.
//!
//! Algorithm: greedy left-to-right walk. Hiragana/katakana characters in the
//! surface act as anchors that must match the reading (katakana is normalized
//! to hiragana first). Non-kana characters (kanji etc.) are accumulated into
//! atomic blocks; the reading range for such a block is determined by the
//! distance between the surrounding anchors.
//!
//! Limitations:
//! - A contiguous kanji compound (e.g. `会議室`) is one atomic segment. You
//!   can re-convert the whole compound but not a sub-portion of it.
//! - When the surface contains non-kana that the reading doesn't account for
//!   (model produced extra characters), alignment may go out of sync.

use crate::kana::katakana_to_hiragana;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKind {
    /// Hiragana/katakana segment with 1:1 character alignment to reading.
    Kana,
    /// Non-kana (kanji etc.) block; the whole block maps to its reading range.
    Kanji,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub surface: String,
    pub reading: String,
    pub kind: SegmentKind,
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}')
}

/// Align the converted `surface` with its original hiragana `reading`.
pub fn align(surface: &str, reading: &str) -> Vec<Segment> {
    let surf_chars: Vec<char> = surface.chars().collect();
    let read_chars: Vec<char> = reading.chars().collect();
    let mut segments = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < surf_chars.len() {
        if is_kana(surf_chars[i]) {
            // Try to match the surface kana against reading[j] (normalize katakana → hiragana).
            let c_str: String = surf_chars[i..=i].iter().collect();
            let c_norm = katakana_to_hiragana(&c_str);
            let c_norm_ch = c_norm.chars().next().unwrap_or(surf_chars[i]);
            if j < read_chars.len() && c_norm_ch == read_chars[j] {
                segments.push(Segment {
                    surface: surf_chars[i].to_string(),
                    reading: read_chars[j].to_string(),
                    kind: SegmentKind::Kana,
                });
                i += 1;
                j += 1;
                continue;
            }
            // Mismatch: fall through and treat as non-kana atomic.
        }

        let start_i = i;
        let start_j = j;
        // Accumulate non-kana (or unmatched kana) until next matching kana anchor.
        while i < surf_chars.len() {
            if is_kana(surf_chars[i]) {
                let c_str: String = surf_chars[i..=i].iter().collect();
                let c_norm = katakana_to_hiragana(&c_str);
                let c_norm_ch = c_norm.chars().next().unwrap_or(surf_chars[i]);
                // Check if this kana appears in the remaining reading (anchor candidate).
                if read_chars[j..].iter().any(|&c| c == c_norm_ch) {
                    break;
                }
            }
            i += 1;
        }

        // Advance j to the matching anchor in reading (if any).
        if i < surf_chars.len() {
            let c_str: String = surf_chars[i..=i].iter().collect();
            let c_norm = katakana_to_hiragana(&c_str);
            let target = c_norm.chars().next().unwrap_or(surf_chars[i]);
            while j < read_chars.len() && read_chars[j] != target {
                j += 1;
            }
        } else {
            j = read_chars.len();
        }

        let surface_segment: String = surf_chars[start_i..i].iter().collect();
        let reading_segment: String = read_chars[start_j..j].iter().collect();

        if !surface_segment.is_empty() {
            segments.push(Segment {
                surface: surface_segment,
                reading: reading_segment,
                kind: SegmentKind::Kanji,
            });
        }
    }

    segments
}

/// Given a character range `[surf_start, surf_end)` in the joined surface
/// (i.e. the concatenation of all segment surfaces), find the corresponding
/// reading range. For ranges that partially overlap a Kanji segment, the
/// segment is expanded to cover the whole block.
///
/// Returns `(reading_start, reading_end, expanded_surface_start, expanded_surface_end)`.
pub fn map_range(
    segments: &[Segment],
    surf_start: usize,
    surf_end: usize,
) -> (usize, usize, usize, usize) {
    let mut s_pos = 0;
    let mut r_pos = 0;
    let mut r_start = None;
    let mut r_end = None;
    let mut new_surf_start = None;
    let mut new_surf_end = None;

    for seg in segments {
        let seg_surf_len = seg.surface.chars().count();
        let seg_read_len = seg.reading.chars().count();
        let s_seg_start = s_pos;
        let s_seg_end = s_pos + seg_surf_len;
        let overlaps = s_seg_start < surf_end && surf_start < s_seg_end;

        if overlaps {
            match seg.kind {
                SegmentKind::Kana => {
                    let local_start = surf_start.saturating_sub(s_seg_start);
                    let local_end = (surf_end - s_seg_start).min(seg_surf_len);
                    if r_start.is_none() {
                        r_start = Some(r_pos + local_start);
                        new_surf_start = Some(s_seg_start + local_start);
                    }
                    r_end = Some(r_pos + local_end);
                    new_surf_end = Some(s_seg_start + local_end);
                }
                SegmentKind::Kanji => {
                    if r_start.is_none() {
                        r_start = Some(r_pos);
                        new_surf_start = Some(s_seg_start);
                    }
                    r_end = Some(r_pos + seg_read_len);
                    new_surf_end = Some(s_seg_end);
                }
            }
        }

        s_pos = s_seg_end;
        r_pos += seg_read_len;
    }

    (
        r_start.unwrap_or(0),
        r_end.unwrap_or(0),
        new_surf_start.unwrap_or(surf_start),
        new_surf_end.unwrap_or(surf_end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments_str(segs: &[Segment]) -> Vec<(String, String)> {
        segs.iter()
            .map(|s| (s.surface.clone(), s.reading.clone()))
            .collect()
    }

    #[test]
    fn align_basic() {
        let segs = align("私はペンです", "わたしはぺんです");
        assert_eq!(
            segments_str(&segs),
            vec![
                ("私".to_string(), "わたし".to_string()),
                ("は".to_string(), "は".to_string()),
                ("ペ".to_string(), "ぺ".to_string()),
                ("ン".to_string(), "ん".to_string()),
                ("で".to_string(), "で".to_string()),
                ("す".to_string(), "す".to_string()),
            ]
        );
    }

    #[test]
    fn align_kanji_compound() {
        // No internal kana anchors → whole compound is one atomic segment.
        let segs = align("会議室", "かいぎしつ");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].surface, "会議室");
        assert_eq!(segs[0].reading, "かいぎしつ");
        assert_eq!(segs[0].kind, SegmentKind::Kanji);
    }

    #[test]
    fn align_okurigana() {
        let segs = align("働きすぎる", "はたらきすぎる");
        // 働 → はたら, き → き, す → す, ぎ → ぎ, る → る
        assert_eq!(segs[0].surface, "働");
        assert_eq!(segs[0].reading, "はたら");
    }

    #[test]
    fn align_repeated_kana() {
        // "は" appears twice in the reading; greedy walk picks the first.
        let segs = align("明日は晴れる", "あしたははれる");
        // 明日 → あした, は → は, 晴 → は, れ → れ, る → る
        assert_eq!(segs[0].surface, "明日");
        assert_eq!(segs[0].reading, "あした");
    }

    #[test]
    fn map_range_kana_only() {
        let segs = align("はとです", "はとです");
        // Select [0..2] = "はと" → reading "はと"
        let (rs, re, ss, se) = map_range(&segs, 0, 2);
        assert_eq!((rs, re), (0, 2));
        assert_eq!((ss, se), (0, 2));
    }

    #[test]
    fn map_range_through_kanji() {
        let segs = align("私はペンです", "わたしはぺんです");
        // Select [2..4] = "ペン" → reading [4..6] = "ぺん"
        let (rs, re, ss, se) = map_range(&segs, 2, 4);
        assert_eq!((rs, re), (4, 6));
        assert_eq!((ss, se), (2, 4));
    }

    #[test]
    fn map_range_expands_into_kanji() {
        let segs = align("私はペンです", "わたしはぺんです");
        // Select only [0..1] = "私" → reading [0..3] = "わたし" (full kanji segment)
        let (rs, re, ss, se) = map_range(&segs, 0, 1);
        assert_eq!((rs, re), (0, 3));
        assert_eq!((ss, se), (0, 1));
    }

    #[test]
    fn map_range_compound_atomic() {
        let segs = align("会議室", "かいぎしつ");
        // Even if user selects just [0..2] = "会議", the whole compound expands.
        let (rs, re, ss, se) = map_range(&segs, 0, 2);
        assert_eq!((rs, re), (0, 5));
        assert_eq!((ss, se), (0, 3));
    }
}

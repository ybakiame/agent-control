//! Full-text search over rules, sessions and evidence.
//!
//! FTS5's unicode61 tokenizer treats a whole CJK run as ONE token (and trigram ignores queries
//! under three characters), so neither segments Chinese. Text is therefore pre-segmented here:
//! CJK runs become character bigrams (zero dictionary), other scripts stay lowercased words.
//! The same transform applies to queries, so a two-character Chinese query matches. Index rows
//! store the space-joined tokens; FTS5 then only has to split on spaces.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    Rules,
    Sessions,
    Evidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: SearchKind,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub project_path: String,
    pub score: f64,
}

pub fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

/// Hybrid segmenter: CJK runs → bigrams (a lone CJK char stays a unigram), everything else →
/// lowercased alphanumeric words.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    for ch in text.chars() {
        if is_cjk(ch) {
            flush_word(&mut word, &mut tokens);
            cjk.push(ch);
        } else if ch.is_alphanumeric() {
            flush_cjk(&mut cjk, &mut tokens);
            word.push(ch);
        } else {
            flush_word(&mut word, &mut tokens);
            flush_cjk(&mut cjk, &mut tokens);
        }
    }
    flush_word(&mut word, &mut tokens);
    flush_cjk(&mut cjk, &mut tokens);
    tokens
}

fn flush_word(word: &mut String, tokens: &mut Vec<String>) {
    if !word.is_empty() {
        tokens.push(word.to_lowercase());
        word.clear();
    }
}

fn flush_cjk(cjk: &mut Vec<char>, tokens: &mut Vec<String>) {
    if cjk.len() == 1 {
        tokens.push(cjk[0].to_string());
    } else {
        for pair in cjk.windows(2) {
            tokens.push(pair.iter().collect());
        }
    }
    cjk.clear();
}

/// Space-joined token form stored in the FTS5 shadow columns.
pub fn index_text(text: &str) -> String {
    tokenize(text).join(" ")
}

/// FTS5 MATCH expression: every token quoted (so CJK bigrams and punctuation are literals) and
/// AND-joined, which makes a multi-bigram CJK query behave as a substring match.
pub fn match_expression(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Rejoin adjacent CJK bigrams in a snippet for display ("文档 档腐 [腐化]" → "文档档[腐化]"),
/// keeping FTS5's `[match]` markers and leaving non-CJK words spaced.
pub fn display_snippet(snippet: &str) -> String {
    fn core(piece: &str) -> &str {
        piece.trim_start_matches('[').trim_end_matches(']')
    }
    fn is_bigram(s: &str) -> bool {
        s.chars().count() == 2 && s.chars().all(is_cjk)
    }
    let mut out = String::new();
    let mut prev: Option<String> = None;
    for piece in snippet.split(' ') {
        if piece.is_empty() {
            continue;
        }
        let c = core(piece);
        let merge = match &prev {
            Some(p) => {
                let pc = core(p);
                is_bigram(pc) && is_bigram(c) && pc.chars().nth(1) == c.chars().next()
            }
            None => false,
        };
        if merge {
            let rest: String = c.chars().skip(1).collect();
            let mut base = out.clone();
            let mut close = "";
            if base.ends_with(']') {
                base.pop();
                close = "]";
            }
            if piece.starts_with('[') {
                // the opening marker moves back over the shared overlap character
                if let Some(x) = base.pop() {
                    base.push('[');
                    base.push(x);
                }
            }
            base.push_str(&rest);
            base.push_str(if piece.ends_with(']') { "]" } else { close });
            out = base;
            let tail: String = out
                .trim_end_matches(']')
                .chars()
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            prev = Some(tail);
        } else {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(piece);
            prev = Some(piece.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_char_chinese_query_is_one_bigram() {
        assert_eq!(tokenize("中文"), vec!["中文"]);
        assert_eq!(match_expression(&tokenize("中文")), "\"中文\"");
    }

    #[test]
    fn longer_chinese_run_segments_to_bigrams() {
        assert_eq!(tokenize("文档腐化"), vec!["文档", "档腐", "腐化"]);
        let doc = index_text("文档腐化检测");
        let expr = match_expression(&tokenize("腐化"));
        assert!(doc.contains("腐化"));
        assert_eq!(expr, "\"腐化\"");
    }

    #[test]
    fn english_and_mixed_text() {
        assert_eq!(tokenize("Hello, World!"), vec!["hello", "world"]);
        assert_eq!(tokenize("mise run 构建"), vec!["mise", "run", "构建"]);
        assert_eq!(tokenize("规则 rule-42"), vec!["规则", "rule", "42"]);
    }

    #[test]
    fn lone_cjk_char_stays_a_unigram() {
        assert_eq!(tokenize("猫"), vec!["猫"]);
    }

    #[test]
    fn snippet_bigrams_rejoin_for_display() {
        assert_eq!(display_snippet("文档 档腐 腐化 化检 检测"), "文档腐化检测");
        assert_eq!(display_snippet("登记 记文 [文档] 档腐"), "登记[文档腐]");
        assert_eq!(
            display_snippet("hello [world] 文档 档腐"),
            "hello [world] 文档腐"
        );
    }

    #[test]
    fn query_tokens_match_index_tokens() {
        let index = index_text("Agent Control 中文搜索 search");
        for query in ["中文", "搜索", "search", "agent"] {
            let expr = match_expression(&tokenize(query));
            assert!(!expr.is_empty(), "{query}");
            assert!(
                expr.trim_matches('"')
                    .split("\" AND \"")
                    .all(|tok| index.split(' ').any(|t| t == tok)),
                "{query} -> {expr} vs {index}"
            );
        }
    }
}

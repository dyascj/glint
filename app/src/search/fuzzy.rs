//! Match scoring for root search. Higher is better; None means no match.
//! Exact > prefix > word starts ("vsc" -> Visual Studio Code) > substring >
//! in-order letters, with shorter titles winning ties.

/// Scores `query` (already lowercased, trimmed) against `title` and extra keywords.
pub fn score(query: &str, title: &str, keywords: &[&str]) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let best = std::iter::once(title).map(|t| one(query, t)).chain(keywords.iter().map(|k| one(query, k).map(|s| s - 120))).flatten().max()?;
    Some(best - title.chars().count().min(60) as i32)
}

fn one(q: &str, text: &str) -> Option<i32> {
    let t = text.to_lowercase();
    if t == q {
        return Some(1000);
    }
    if t.starts_with(q) {
        return Some(900);
    }
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| w.starts_with(q)) {
        return Some(800);
    }
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if initials.starts_with(q) && q.chars().count() >= 2 {
        return Some(760);
    }
    // Camel case initials too: "gh" -> GitHub Desktop.
    let caps: String = text.split_whitespace().flat_map(|w| w.chars().take(1).chain(w.chars().skip(1).filter(|c| c.is_uppercase()))).collect::<String>().to_lowercase();
    if caps.starts_with(q) && q.chars().count() >= 2 {
        return Some(740);
    }
    if t.contains(q) {
        return Some(600);
    }
    subsequence(q, &t)
}

/// Letters in order with gaps; tighter runs score higher.
fn subsequence(q: &str, t: &str) -> Option<i32> {
    let tc: Vec<char> = t.chars().collect();
    let mut i = 0;
    let mut gaps = 0;
    let mut last: Option<usize> = None;
    for qc in q.chars() {
        if qc.is_whitespace() {
            continue;
        }
        let found = tc[i..].iter().position(|&c| c == qc)? + i;
        if let Some(l) = last {
            gaps += (found - l - 1) as i32;
        }
        last = Some(found);
        i = found + 1;
    }
    if q.chars().count() < 2 {
        return None;
    }
    Some((400 - gaps * 12).max(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_like_a_launcher() {
        let s = |q: &str, t: &str| score(q, t, &[]);
        assert!(s("safari", "Safari") > s("saf", "Safari"));
        assert!(s("saf", "Safari") > s("saf", "Microsoft Safety"));
        assert!(s("vsc", "Visual Studio Code").is_some());
        assert!(s("code", "Visual Studio Code") > s("code", "Xcode"), "word start beats substring");
        assert!(s("gh", "GitHub Desktop").is_some());
        assert!(s("ntn", "Notion").is_some());
        assert!(s("zzz", "Notion").is_none());
        assert!(s("x", "Notion").is_none(), "one stray letter is not a match");
        assert!(s("set", "Settings") > s("set", "System Settings"), "shorter wins ties");
        assert!(score("prefs", "System Settings", &["preferences"]).is_some(), "keywords match");
        assert_eq!(s("", "Anything"), Some(0));
    }
}

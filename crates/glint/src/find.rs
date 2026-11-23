//! Forgiving text matching for the filters inside the app: the library, a playlist's search,
//! Settings. A query still finds a title when it has a typo, skips letters, was typed on the
//! other keyboard layout, or spells a Russian name in Latin letters (and the other way round).
//!
//! Both sides are folded to one plain Latin spelling first: "Земфира", "zemfira" and "Zemfíra"
//! all become `zemfira`. Then every word of the query has to turn up in the title, in any order.

/// A query, folded once and tested against many titles.
pub struct Query {
    forms: Vec<Form>,
}

/// One way to read the query: as typed, or as if typed on the other keyboard layout.
struct Form {
    words: Vec<Vec<u8>>,
    /// Near misses count for the query as typed. The other layout has to match as it is: its
    /// letters are a guess, and a guess with typos on top finds nonsense.
    fuzzy: bool,
}

/// How a title matched: 0 as typed, and the more letters off, the higher. Lower is better.
pub type Tier = u8;
const EXACT: Tier = 0;

impl Query {
    /// None when there is nothing to look for.
    pub fn new(text: &str) -> Option<Query> {
        let words = split(&fold(text));
        if words.is_empty() {
            return None;
        }
        let mut forms = vec![Form { words, fuzzy: true }];
        if let Some(other) = other_layout(text) {
            let words = split(&fold(&other));
            if !words.is_empty() && forms.iter().all(|f| f.words != words) {
                forms.push(Form { words, fuzzy: false });
            }
        }
        Some(Query { forms })
    }

    /// How well `fields` (a title, its artists, its album) match, or None when they don't.
    pub fn tier(&self, fields: &[&str]) -> Option<Tier> {
        let mut hay = String::new();
        for f in fields {
            hay.push_str(&fold(f));
            hay.push(' ');
        }
        let hay = Hay::new(&hay);
        self.forms.iter().filter_map(|f| f.tier(&hay)).min()
    }

    pub fn matches(&self, fields: &[&str]) -> bool {
        self.tier(fields).is_some()
    }

    /// Whether `fields` hold every word of the query as typed, near misses aside.
    pub fn exact(&self, fields: &[&str]) -> bool {
        self.tier(fields) == Some(EXACT)
    }
}

/// The indices of the items that matched, only the best tier: once something matches as typed,
/// the near misses stay out of the way.
pub fn best(tiers: impl IntoIterator<Item = Option<Tier>>) -> Vec<usize> {
    let all: Vec<(usize, Tier)> = tiers.into_iter().enumerate().filter_map(|(i, t)| Some((i, t?))).collect();
    let Some(top) = all.iter().map(|&(_, t)| t).min() else { return Vec::new() };
    all.into_iter().filter(|&(_, t)| t == top).map(|(i, _)| i).collect()
}

/// The text as if it had been typed on the other keyboard layout (ЙЦУКЕН and QWERTY), or None
/// when that changes nothing.
pub fn other_layout(text: &str) -> Option<String> {
    const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
    const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";
    let ru: Vec<char> = RU.chars().collect();
    let en: Vec<char> = EN.chars().collect();
    let cyrillic = text.chars().any(|c| ru.contains(&c.to_lowercase().next().unwrap_or(c)));
    let (from, to) = if cyrillic { (&ru, &en) } else { (&en, &ru) };
    let out: String = text
        .chars()
        .map(|c| {
            let low = c.to_lowercase().next().unwrap_or(c);
            match from.iter().position(|&x| x == low) {
                Some(i) if c != low => to[i].to_uppercase().next().unwrap_or(to[i]),
                Some(i) => to[i],
                None => c,
            }
        })
        .collect();
    (out != text).then_some(out)
}

impl Form {
    fn tier(&self, hay: &Hay) -> Option<Tier> {
        let mut worst = EXACT;
        for w in &self.words {
            let t = if hay.contains(w) {
                EXACT
            } else if self.fuzzy
                && let Some(off) = hay.near(w)
            {
                off
            } else {
                return None;
            };
            worst = worst.max(t);
        }
        Some(worst)
    }
}

/// A folded title: its words, and all of it with the spaces taken out, so "pinkfloyd" finds
/// "Pink Floyd".
struct Hay {
    text: Vec<u8>,
    joined: Vec<u8>,
    words: Vec<Vec<u8>>,
}

impl Hay {
    fn new(folded: &str) -> Hay {
        let words = split(folded);
        Hay { text: folded.as_bytes().to_vec(), joined: words.concat(), words }
    }

    fn contains(&self, w: &[u8]) -> bool {
        find(&self.text, w) || find(&self.joined, w)
    }

    /// A near miss: the word starts like one of the title's words with a few letters wrong,
    /// missing, extra or swapped, or it is that word with letters left out. Gives how many
    /// letters it is off by; letters left out count as one more than the most typos allowed.
    fn near(&self, w: &[u8]) -> Option<Tier> {
        let n = w.len();
        if n < 3 {
            return None;
        }
        let k = match n {
            3 => 0,
            4..=5 => 1,
            6..=8 => 2,
            _ => 3,
        };
        let typo = if k > 0 { self.words.iter().map(|h| prefix_distance(w, h, k)).filter(|&d| d <= k).min() } else { None };
        let gap = self.words.iter().any(|h| skipped(w, h)).then_some(k + 1);
        typo.or(gap).map(|d| d as Tier)
    }
}

fn find(hay: &[u8], w: &[u8]) -> bool {
    w.is_empty() || hay.windows(w.len()).any(|x| x == w)
}

/// `w` is `h` with letters left out: same first letter, every letter of `w` in `h` in order,
/// and at least half of `h` is there.
fn skipped(w: &[u8], h: &[u8]) -> bool {
    if w.len() < 3 || w.first() != h.first() || w.len() * 2 < h.len() {
        return false;
    }
    let mut it = h.iter();
    w.iter().all(|c| it.any(|x| x == c))
}

/// The smallest edit distance between `w` and any start of `h`, counting a swap of two
/// neighbouring letters as one edit. Stops early once it can only be over `k`.
fn prefix_distance(w: &[u8], h: &[u8], k: usize) -> usize {
    let (m, n) = (w.len(), h.len());
    let mut prev2 = vec![0usize; n + 1];
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=n {
            let cost = usize::from(w[i - 1] != h[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && w[i - 1] == h[j - 2] && w[i - 2] == h[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
            row_min = row_min.min(d);
        }
        if row_min > k {
            return row_min;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.iter().copied().min().unwrap_or(m)
}

fn split(folded: &str) -> Vec<Vec<u8>> {
    folded.split(' ').filter(|w| !w.is_empty()).map(|w| w.as_bytes().to_vec()).collect()
}

/// Lowercase plain Latin letters, digits and single spaces. Cyrillic is spelled in Latin,
/// accents are dropped, and spellings that sound alike are made the same ("kh" and "h",
/// "ts" and "c", "ja" and "ya"), so either way of writing a name folds to one.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let s: &str = match c {
            'a'..='z' | '0'..='9' => {
                out.push(c);
                continue;
            }
            'а' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' | 'ё' | 'э' | 'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'ж' => "zh",
            'з' | 'ž' | 'ź' | 'ż' => "z",
            'и' | 'і' | 'ì' | 'í' | 'î' | 'ï' => "i",
            'й' | 'ы' | 'ý' | 'ÿ' => "y",
            'к' => "k",
            'л' | 'ł' => "l",
            'м' => "m",
            'н' | 'ñ' | 'ń' => "n",
            'о' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
            'п' => "p",
            'р' => "r",
            'с' | 'š' | 'ś' => "s",
            'т' => "t",
            'у' | 'ù' | 'ú' | 'û' | 'ü' => "u",
            'ф' => "f",
            'х' => "h",
            'ц' | 'ç' | 'č' | 'ć' => "c",
            'ч' => "ch",
            'ш' | 'щ' => "sh",
            'ю' => "yu",
            'я' => "ya",
            'ь' | 'ъ' | '\'' | '’' | '`' => "",
            'ß' => "ss",
            _ => " ",
        };
        out.push_str(s);
    }
    // A "y" after a vowel is the short "й" sound however it is written: "trey", "trei" and "трей"
    // all end the same.
    let out = {
        let b = out.as_bytes();
        let mut o = String::with_capacity(out.len());
        for (i, &c) in b.iter().enumerate() {
            let after_vowel = i > 0 && b"aeiou".contains(&b[i - 1]);
            let before_vowel = b.get(i + 1).is_some_and(|n| b"aeiou".contains(n));
            o.push(if c == b'y' && after_vowel && !before_vowel { 'i' } else { c as char });
        }
        o
    };
    let mut words: Vec<String> = Vec::new();
    for w in out.split(' ').filter(|w| !w.is_empty()) {
        words.push(sound_alike(w));
    }
    words.join(" ")
}

fn sound_alike(w: &str) -> String {
    const SAME: [(&str, &str); 14] = [
        ("shch", "sh"),
        ("sch", "sh"),
        ("tch", "ch"),
        ("kh", "h"),
        ("ts", "c"),
        ("tz", "c"),
        ("ph", "f"),
        ("ck", "k"),
        ("ja", "ya"),
        ("ju", "yu"),
        ("x", "ks"),
        ("w", "v"),
        ("q", "k"),
        ("j", "y"),
    ];
    let mut s = w.to_owned();
    for (from, to) in SAME {
        if s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s
}

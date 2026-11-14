//! Release versions, ordered the way semver orders them: `0.1.0-alpha` comes before
//! `0.1.0-alpha.1`, which comes before `0.1.0-beta`, which comes before `0.1.0`.

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Text(String),
}

impl Ord for Ident {
    fn cmp(&self, other: &Ident) -> Ordering {
        match (self, other) {
            (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
            (Ident::Num(_), Ident::Text(_)) => Ordering::Less,
            (Ident::Text(_), Ident::Num(_)) => Ordering::Greater,
            (Ident::Text(a), Ident::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, other: &Ident) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Ident>,
}

impl Version {
    /// `1.2.3`, `v1.2.3-beta.2` or `1.2.3+build`; anything else is `None`.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        let text = text.split_once('+').map_or(text, |(v, _)| v);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut nums = core.split('.').map(|p| p.parse::<u64>().ok());
        let version = [nums.next()??, nums.next()??, nums.next()??];
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => p
                .split('.')
                .map(|id| match id {
                    "" => None,
                    _ if id.bytes().all(|b| b.is_ascii_digit()) => id.parse().ok().map(Ident::Num),
                    _ if id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => Some(Ident::Text(id.to_owned())),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Version { core: version, pre })
    }

    /// Whether this is a pre-release (`-alpha`, `-rc.1`).
    pub fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A release is newer than any pre-release of it.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self.pre.cmp(&other.pre),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} should parse"))
    }

    #[test]
    fn orders_like_semver() {
        let order = ["0.1.0-alpha", "0.1.0-alpha.1", "0.1.0-alpha.2", "0.1.0-alpha.10", "0.1.0-beta", "0.1.0-rc.1", "0.1.0", "0.1.1", "0.2.0", "1.0.0"];
        for pair in order.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn a_leading_v_and_build_metadata_are_ignored() {
        assert_eq!(v("v0.1.0-alpha"), v("0.1.0-alpha"));
        assert_eq!(v("1.2.3+abc"), v("1.2.3"));
    }

    #[test]
    fn junk_is_refused() {
        for bad in ["", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "1.2.3-a..b", "latest", "1.2.3-é"] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn knows_a_pre_release() {
        assert!(v("0.1.0-alpha").is_pre());
        assert!(!v("0.1.0").is_pre());
    }
}

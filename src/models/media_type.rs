use std::fmt;
use std::str::FromStr;

/// The type of barcode/code scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeType {
    Isbn,
    Upc,
    Issn,
}

impl fmt::Display for CodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodeType::Isbn => write!(f, "isbn"),
            CodeType::Upc => write!(f, "upc"),
            CodeType::Issn => write!(f, "issn"),
        }
    }
}

/// Media types supported by the library catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Book,
    Bd,
    Cd,
    Dvd,
    Magazine,
    Report,
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaType::Book => write!(f, "book"),
            MediaType::Bd => write!(f, "bd"),
            MediaType::Cd => write!(f, "cd"),
            MediaType::Dvd => write!(f, "dvd"),
            MediaType::Magazine => write!(f, "magazine"),
            MediaType::Report => write!(f, "report"),
        }
    }
}

impl FromStr for MediaType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "book" => Ok(MediaType::Book),
            "bd" => Ok(MediaType::Bd),
            "cd" => Ok(MediaType::Cd),
            "dvd" => Ok(MediaType::Dvd),
            "magazine" => Ok(MediaType::Magazine),
            "report" => Ok(MediaType::Report),
            _ => Err(format!("Unknown media type: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_lowercase() {
        assert_eq!(MediaType::Book.to_string(), "book");
        assert_eq!(MediaType::Bd.to_string(), "bd");
        assert_eq!(MediaType::Cd.to_string(), "cd");
        assert_eq!(MediaType::Dvd.to_string(), "dvd");
        assert_eq!(MediaType::Magazine.to_string(), "magazine");
        assert_eq!(MediaType::Report.to_string(), "report");
    }

    #[test]
    fn test_from_str_roundtrip() {
        for mt in [
            MediaType::Book,
            MediaType::Bd,
            MediaType::Cd,
            MediaType::Dvd,
            MediaType::Magazine,
            MediaType::Report,
        ] {
            let s = mt.to_string();
            let parsed: MediaType = s.parse().unwrap();
            assert_eq!(parsed, mt);
        }
    }

    #[test]
    fn test_from_str_case_insensitive() {
        assert_eq!("BOOK".parse::<MediaType>().unwrap(), MediaType::Book);
        assert_eq!("Book".parse::<MediaType>().unwrap(), MediaType::Book);
        assert_eq!("DVD".parse::<MediaType>().unwrap(), MediaType::Dvd);
    }

    #[test]
    fn test_from_str_unknown() {
        assert!("vinyl".parse::<MediaType>().is_err());
    }

    #[test]
    fn test_code_type_display() {
        assert_eq!(CodeType::Isbn.to_string(), "isbn");
        assert_eq!(CodeType::Upc.to_string(), "upc");
        assert_eq!(CodeType::Issn.to_string(), "issn");
    }
}

//! Godot's string-to-number rules, as the GDScript runtime applies them.
//!
//! `DictionaryFromXml.gd` types every XML attribute with `String.is_valid_int()` /
//! `is_valid_float()` and converts with `int()` / `float()`; reproducing the dictionary it
//! builds needs the same rules, which differ from Rust's `str::parse` (`"1e"` is a valid float,
//! `int("2.5")` is 2, `float("12abc")` is 12).

/// `String.is_valid_int()`: an optional sign (only when more than one character) then digits.
pub fn is_valid_int(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    let from = usize::from(b.len() != 1 && (b[0] == b'+' || b[0] == b'-'));
    b[from..].iter().all(u8::is_ascii_digit)
}

/// `String.is_valid_float()`: an optional sign, digits with at most one period, an optional
/// lowercase `e` exponent with an optional sign; at least one digit before the exponent.
pub fn is_valid_float(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    let from = usize::from(b[0] == b'+' || b[0] == b'-');
    let (mut exponent, mut period, mut sign, mut exp_values, mut numbers) =
        (false, false, false, false, false);
    for &c in &b[from..] {
        if c.is_ascii_digit() {
            if exponent {
                exp_values = true;
            } else {
                numbers = true;
            }
        } else if numbers && !exponent && c == b'e' {
            exponent = true;
        } else if !period && !exponent && c == b'.' {
            period = true;
        } else if (c == b'-' || c == b'+') && exponent && !exp_values && !sign {
            sign = true;
        } else {
            return false;
        }
    }
    numbers
}

/// `String.to_int()` (GDScript `int(string)`): digits up to the first period, other characters
/// skipped, a `-` before the first digit flips the sign; clamps on overflow.
pub fn to_int(s: &str) -> i64 {
    let to = s.find('.').unwrap_or(s.len());
    let mut integer: i64 = 0;
    let mut sign: i64 = 1;
    for c in s[..to].bytes() {
        if c.is_ascii_digit() {
            let d = i64::from(c - b'0');
            match integer.checked_mul(10).and_then(|v| v.checked_add(d)) {
                Some(v) => integer = v,
                None => return if sign == 1 { i64::MAX } else { i64::MIN },
            }
        } else if integer == 0 && c == b'-' {
            sign = -sign;
        }
    }
    integer * sign
}

/// `String.to_float()` (GDScript `float(string)`): leading white space skipped, then the
/// longest prefix that reads as a decimal number; 0 when there is none.
pub fn to_float(s: &str) -> f64 {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut any_digits = i > digits_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        any_digits |= i > frac;
    }
    if !any_digits {
        return 0.0;
    }
    let mantissa_end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_digits = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_digits {
            i = j;
        }
    }
    let end = if i > mantissa_end { i } else { mantissa_end };
    t[..end].parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints() {
        assert!(
            is_valid_int("42") && is_valid_int("-3") && is_valid_int("+7") && is_valid_int("007")
        );
        assert!(
            !is_valid_int("") && !is_valid_int("-") && !is_valid_int("1.0") && !is_valid_int(" 5")
        );
        assert_eq!(to_int("2.5"), 2);
        assert_eq!(to_int("abc"), 0);
        assert_eq!(to_int("-3x4"), -34);
        assert_eq!(to_int("+7"), 7);
        assert_eq!(to_int("4294967295"), 4_294_967_295);
    }

    #[test]
    fn floats() {
        assert!(is_valid_float("1e") && is_valid_float("1.") && is_valid_float(".5"));
        assert!(is_valid_float("-.5") && is_valid_float("1e+") && is_valid_float("1e-05"));
        assert!(!is_valid_float(".") && !is_valid_float("1E5") && !is_valid_float("#ff0000"));
        assert_eq!(to_float("1e5x"), 100000.0);
        assert_eq!(to_float(" 12"), 12.0);
        assert_eq!(to_float("\n12"), 12.0);
        assert_eq!(to_float("1e"), 1.0);
        assert_eq!(to_float("0x10"), 0.0);
        assert_eq!(to_float("."), 0.0);
        assert_eq!(to_float("2.5"), 2.5);
    }
}

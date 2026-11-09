/// Digits and layout of a float as the native `float.to_text` prints it: the shortest text that
/// reads back as the same number, plain between 1e-4 and 1e16, exponent form outside.
pub fn float_text(f: f64) -> String {
    layout_float(f.is_nan(), f.is_sign_negative(), f.is_infinite(), f == 0.0, || format!("{:e}", f.abs()))
}

/// The same for an `f32` value.
pub fn float_text_f32(f: f32) -> String {
    layout_float(f.is_nan(), f.is_sign_negative(), f.is_infinite(), f == 0.0, || format!("{:e}", f.abs()))
}

fn layout_float(nan: bool, neg: bool, inf: bool, zero: bool, sci: impl Fn() -> String) -> String {
    if nan {
        return "NaN".into();
    }
    let sign = if neg { "-" } else { "" };
    if inf {
        return format!("{}inf", sign);
    }
    if zero {
        return format!("{}0.0", sign);
    }
    let sci = sci();
    let (mant, exp) = sci.split_once('e').unwrap();
    let x: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    if !(-4..16).contains(&x) {
        let rest = &digits[1..];
        let m = if rest.is_empty() { digits[..1].to_string() } else { format!("{}.{}", &digits[..1], rest) };
        return format!("{}{}e{}", sign, m, x);
    }
    if x < 0 {
        return format!("{}0.{}{}", sign, "0".repeat((-x - 1) as usize), digits);
    }
    if n <= x + 1 {
        return format!("{}{}{}.0", sign, digits, "0".repeat((x + 1 - n) as usize));
    }
    format!("{}{}.{}", sign, &digits[..(x + 1) as usize], &digits[(x + 1) as usize..])
}

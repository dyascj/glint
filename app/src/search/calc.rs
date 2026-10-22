//! Inline calculator: arithmetic, functions, percentages and unit conversion
//! ("3 ft to cm", "72f in c", "24px to rem"). A small recursive-descent parser.

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// Display form with thousands separators ("1,234.5 cm").
    pub text: String,
    /// What gets copied ("1234.5").
    pub raw: String,
    /// "Centimetres", "Tokyo · Fri" etc.; None for plain numbers.
    pub unit: Option<String>,
}

/// None unless the query is clearly a calculation (an operator, function or
/// conversion); a bare number isn't.
pub fn evaluate(query: &str) -> Option<Answer> {
    let q = query.trim().to_lowercase();
    if q.is_empty() || q.len() > 200 {
        return None;
    }
    if let Some(a) = convert(&q) {
        return Some(a);
    }
    if let Some(a) = super::time::evaluate(&q) {
        return Some(a);
    }
    let tokens = lex(&q)?;
    let interesting = tokens.iter().any(|t| matches!(t, Tok::Op(_) | Tok::Ident(_) | Tok::Percent)) && tokens.len() > 1;
    if !interesting {
        return None;
    }
    let mut p = Parser { t: &tokens, i: 0 };
    let v = p.expr()?;
    if p.i != tokens.len() || !v.is_finite() {
        return None;
    }
    Some(Answer { text: pretty(v), raw: plain(v), unit: None })
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Op(char),
    LParen,
    RParen,
    Comma,
    Percent,
    Ident(String),
}

fn lex(s: &str) -> Option<Vec<Tok>> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            // A comma followed by exactly three digits is a thousands separator.
            let grouping = |i: usize| {
                c[i] == ',' && c.get(i + 1..i + 4).is_some_and(|w| w.iter().all(char::is_ascii_digit)) && c.get(i + 4).is_none_or(|d| !d.is_ascii_digit())
            };
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || grouping(i)) {
                i += 1;
            }
            let n: String = c[start..i].iter().filter(|&&d| d != ',').collect();
            out.push(Tok::Num(n.parse().ok()?));
        } else if ch.is_alphabetic() {
            let start = i;
            while i < c.len() && c[i].is_alphanumeric() {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            // "3 x 4" means times when it sits between numbers.
            if word == "x" && matches!(out.last(), Some(Tok::Num(_) | Tok::RParen)) {
                out.push(Tok::Op('*'));
            } else {
                out.push(Tok::Ident(word));
            }
        } else {
            out.push(match ch {
                '+' | '-' | '*' | '/' | '^' => Tok::Op(ch),
                '×' => Tok::Op('*'),
                '÷' => Tok::Op('/'),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                '%' => Tok::Percent,
                _ => return None,
            });
            i += 1;
        }
    }
    Some(out)
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// Sums. "a + b%" adds b percent of a, like a calculator does.
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let start = self.i;
            let r = self.term()?;
            let pct = self.t[start..self.i].last() == Some(&Tok::Percent) && self.i - start == 2;
            let r = if pct { v * r } else { r };
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(Tok::Op('/')) => {
                    self.i += 1;
                    v /= self.power()?;
                }
                Some(Tok::Ident(w)) if w == "mod" => {
                    self.i += 1;
                    v %= self.power()?;
                }
                // "20% of 150"
                Some(Tok::Ident(w)) if w == "of" => {
                    self.i += 1;
                    v *= self.power()?;
                }
                _ => return Some(v),
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(&Tok::Op('^')) {
            let exp = self.power()?;
            return Some(base.powf(exp));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(&Tok::Op('-')) {
            return Some(-self.unary()?);
        }
        if self.eat(&Tok::Op('+')) {
            return self.unary();
        }
        let mut v = self.atom()?;
        if self.eat(&Tok::Percent) {
            v /= 100.0;
        }
        Some(v)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek().cloned()? {
            Tok::Num(n) => {
                self.i += 1;
                Some(n)
            }
            Tok::LParen => {
                self.i += 1;
                let v = self.expr()?;
                self.eat(&Tok::RParen).then_some(v)
            }
            Tok::Ident(name) => {
                self.i += 1;
                match name.as_str() {
                    "pi" | "π" => return Some(std::f64::consts::PI),
                    "e" => return Some(std::f64::consts::E),
                    "tau" => return Some(std::f64::consts::TAU),
                    _ => {}
                }
                let mut args = Vec::new();
                if self.eat(&Tok::LParen) {
                    if !self.eat(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(&Tok::RParen) {
                                break;
                            }
                            if !self.eat(&Tok::Comma) {
                                return None;
                            }
                        }
                    }
                } else {
                    args.push(self.unary()?);
                }
                func(&name, &args)
            }
            _ => None,
        }
    }
}

fn func(name: &str, a: &[f64]) -> Option<f64> {
    let one = || (a.len() == 1).then(|| a[0]);
    Some(match name {
        "sqrt" => one()?.sqrt(),
        "cbrt" => one()?.cbrt(),
        "abs" => one()?.abs(),
        "round" => one()?.round(),
        "floor" => one()?.floor(),
        "ceil" => one()?.ceil(),
        "ln" => one()?.ln(),
        "log" => one()?.log10(),
        "log2" => one()?.log2(),
        "exp" => one()?.exp(),
        "sin" => one()?.sin(),
        "cos" => one()?.cos(),
        "tan" => one()?.tan(),
        "asin" => one()?.asin(),
        "acos" => one()?.acos(),
        "atan" => one()?.atan(),
        "min" if !a.is_empty() => a.iter().copied().fold(f64::INFINITY, f64::min),
        "max" if !a.is_empty() => a.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => return None,
    })
}

// ---- units ----

#[derive(Clone, Copy, PartialEq)]
enum Dim {
    Length,
    Mass,
    Temp,
    Data,
    Time,
    Volume,
    Css,
}

/// (names, dimension, factor to the base unit, display name)
const UNITS: &[(&[&str], Dim, f64, &str)] = &[
    (&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Dim::Length, 0.001, "Millimetres"),
    (&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Dim::Length, 0.01, "Centimetres"),
    (&["m", "meter", "meters", "metre", "metres"], Dim::Length, 1.0, "Metres"),
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Dim::Length, 1000.0, "Kilometres"),
    (&["in", "inch", "inches"], Dim::Length, 0.0254, "Inches"),
    (&["ft", "foot", "feet"], Dim::Length, 0.3048, "Feet"),
    (&["yd", "yard", "yards"], Dim::Length, 0.9144, "Yards"),
    (&["mi", "mile", "miles"], Dim::Length, 1609.344, "Miles"),
    (&["mg", "milligram", "milligrams"], Dim::Mass, 0.001, "Milligrams"),
    (&["g", "gram", "grams"], Dim::Mass, 1.0, "Grams"),
    (&["kg", "kilogram", "kilograms", "kilo", "kilos"], Dim::Mass, 1000.0, "Kilograms"),
    (&["oz", "ounce", "ounces"], Dim::Mass, 28.349523125, "Ounces"),
    (&["lb", "lbs", "pound", "pounds"], Dim::Mass, 453.59237, "Pounds"),
    (&["st", "stone"], Dim::Mass, 6350.29318, "Stone"),
    (&["c", "celsius", "°c"], Dim::Temp, 0.0, "Celsius"),
    (&["f", "fahrenheit", "°f"], Dim::Temp, 0.0, "Fahrenheit"),
    (&["k", "kelvin"], Dim::Temp, 0.0, "Kelvin"),
    (&["b", "byte", "bytes"], Dim::Data, 1.0, "Bytes"),
    (&["kb", "kilobyte", "kilobytes"], Dim::Data, 1e3, "Kilobytes"),
    (&["mb", "megabyte", "megabytes"], Dim::Data, 1e6, "Megabytes"),
    (&["gb", "gigabyte", "gigabytes"], Dim::Data, 1e9, "Gigabytes"),
    (&["tb", "terabyte", "terabytes"], Dim::Data, 1e12, "Terabytes"),
    (&["kib"], Dim::Data, 1024.0, "Kibibytes"),
    (&["mib"], Dim::Data, 1048576.0, "Mebibytes"),
    (&["gib"], Dim::Data, 1073741824.0, "Gibibytes"),
    (&["ms", "millisecond", "milliseconds"], Dim::Time, 0.001, "Milliseconds"),
    (&["s", "sec", "secs", "second", "seconds"], Dim::Time, 1.0, "Seconds"),
    (&["min", "mins", "minute", "minutes"], Dim::Time, 60.0, "Minutes"),
    (&["h", "hr", "hrs", "hour", "hours"], Dim::Time, 3600.0, "Hours"),
    (&["d", "day", "days"], Dim::Time, 86400.0, "Days"),
    (&["wk", "week", "weeks"], Dim::Time, 604800.0, "Weeks"),
    (&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], Dim::Volume, 0.001, "Millilitres"),
    (&["l", "liter", "liters", "litre", "litres"], Dim::Volume, 1.0, "Litres"),
    (&["cup", "cups"], Dim::Volume, 0.2365882365, "Cups"),
    (&["floz"], Dim::Volume, 0.0295735295625, "Fluid ounces"),
    (&["gal", "gallon", "gallons"], Dim::Volume, 3.785411784, "Gallons"),
    (&["px", "pixel", "pixels"], Dim::Css, 1.0, "Pixels"),
    (&["rem", "em"], Dim::Css, 16.0, "Rem"),
    (&["pt", "point", "points"], Dim::Css, 4.0 / 3.0, "Points"),
];

fn unit(name: &str) -> Option<&'static (&'static [&'static str], Dim, f64, &'static str)> {
    UNITS.iter().find(|u| u.0.contains(&name))
}

/// "<expr> <unit> to|in|as <unit>", also with the unit glued on ("5km in mi").
fn convert(q: &str) -> Option<Answer> {
    let (lhs, to) = [" to ", " in ", " as ", " -> ", " => "].iter().find_map(|sep| q.rsplit_once(sep))?;
    let to = unit(to.trim())?;
    let lhs = lhs.trim();
    let split = lhs.rfind(|c: char| c.is_ascii_digit() || c == ')' || c == ' ')?;
    let (num, from) = lhs.split_at(split + 1);
    let from = unit(from.trim())?;
    if from.1 != to.1 {
        return None;
    }
    let v = number(num.trim())?;
    let out = if from.1 == Dim::Temp {
        let c = match from.3 {
            "Fahrenheit" => (v - 32.0) * 5.0 / 9.0,
            "Kelvin" => v - 273.15,
            _ => v,
        };
        match to.3 {
            "Fahrenheit" => c * 9.0 / 5.0 + 32.0,
            "Kelvin" => c + 273.15,
            _ => c,
        }
    } else {
        v * from.2 / to.2
    };
    let short = to.0[0];
    Some(Answer { text: format!("{} {short}", pretty(out)), raw: plain(out), unit: Some(to.3.to_string()) })
}

fn number(s: &str) -> Option<f64> {
    s.parse().ok().or_else(|| {
        let tokens = lex(s)?;
        let mut p = Parser { t: &tokens, i: 0 };
        let v = p.expr()?;
        (p.i == tokens.len()).then_some(v)
    })
}

/// Up to 10 decimals, trailing zeros trimmed.
fn plain(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    if v.abs() >= 1e15 || v.abs() < 1e-9 {
        return format!("{v:e}");
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn pretty(v: f64) -> String {
    let s = plain(v);
    if s.contains('e') {
        return s;
    }
    let (sign, s) = s.strip_prefix('-').map_or(("", s.as_str()), |r| ("-", r));
    let (int, frac) = s.split_once('.').map_or((s, None), |(i, f)| (i, Some(f)));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(q: &str) -> String {
        evaluate(q).map(|a| a.raw).unwrap_or_else(|| "none".into())
    }

    #[test]
    fn arithmetic() {
        assert_eq!(val("2+2"), "4");
        assert_eq!(val("2 + 3 * 4"), "14");
        assert_eq!(val("(2 + 3) * 4"), "20");
        assert_eq!(val("2^10"), "1024");
        assert_eq!(val("2^3^2"), "512", "power is right-associative");
        assert_eq!(val("-3 + 5"), "2");
        assert_eq!(val("10 / 4"), "2.5");
        assert_eq!(val("3 x 4"), "12");
        assert_eq!(val("1,000 * 3"), "3000");
        assert_eq!(val("sqrt(16) + 1"), "5");
        assert_eq!(val("max(1, 7, 3)"), "7");
        assert_eq!(val("17 mod 5"), "2");
        assert_eq!(val("1/3"), "0.3333333333");
    }

    #[test]
    fn percentages() {
        assert_eq!(val("100 + 10%"), "110");
        assert_eq!(val("80 - 25%"), "60");
        assert_eq!(val("20% of 150"), "30");
        assert_eq!(val("50 * 10%"), "5");
    }

    #[test]
    fn not_calculations() {
        assert_eq!(val("42"), "none");
        assert_eq!(val("safari"), "none");
        assert_eq!(val("2 +"), "none");
        assert_eq!(val("1/0"), "none");
        assert_eq!(val("visual studio"), "none");
    }

    #[test]
    fn conversions() {
        assert_eq!(val("1 km to m"), "1000");
        assert_eq!(val("3 ft in cm"), "91.44");
        assert_eq!(val("5km in mi"), "3.1068559612");
        assert_eq!(val("212 f to c"), "100");
        assert_eq!(val("0 c in k"), "273.15");
        assert_eq!(val("24px to rem"), "1.5");
        assert_eq!(val("2 gb to mb"), "2000");
        assert_eq!(val("90 min to hours"), "1.5");
        assert_eq!(val("3 kg to feet"), "none", "different dimensions");
        assert_eq!(evaluate("1500 m to km").unwrap().text, "1.5 km");
        assert_eq!(evaluate("1234567 * 1").unwrap().text, "1,234,567");
    }
}

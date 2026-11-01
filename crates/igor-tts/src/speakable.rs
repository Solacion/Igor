//! Rewrites text written for reading into words for speaking.
//!
//! espeak-ng reads `9.1 km/h` as "nine point one k m slash h", `26.6°C` as "twenty-six point six
//! degrees C", `R125` as "R one hundred and twenty-five" and `#2241` as "hash 2241". [`speakable`]
//! turns a unit that follows a number into words ("9.1 kilometres per hour", "26.6 degrees"), an
//! amount into words ("125 rand"), `#` before a number into "number", a month abbreviation next to
//! a number into the month ("October 26") and an ISO date into a spoken date ("26 October 2026").
//! Only text next to a number is rewritten, so ordinary words are left alone.

/// A unit as written after a number, and how to say it.
struct Unit {
    written: &'static str,
    one: &'static str,
    many: &'static str,
}

const fn unit(written: &'static str, one: &'static str, many: &'static str) -> Unit {
    Unit { written, one, many }
}

/// Units to say, longest first where one is a prefix of another. Celsius is plain "degrees", as
/// people say it; Fahrenheit is named.
const UNITS: &[Unit] = &[
    unit("°C", "degree", "degrees"),
    unit("ºC", "degree", "degrees"),
    unit("°F", "degree Fahrenheit", "degrees Fahrenheit"),
    unit("ºF", "degree Fahrenheit", "degrees Fahrenheit"),
    unit("°", "degree", "degrees"),
    unit("º", "degree", "degrees"),
    unit("km/h", "kilometre per hour", "kilometres per hour"),
    unit("kmh", "kilometre per hour", "kilometres per hour"),
    unit("kph", "kilometre per hour", "kilometres per hour"),
    unit("mph", "mile per hour", "miles per hour"),
    unit("m/s", "metre per second", "metres per second"),
    unit("kWh", "kilowatt hour", "kilowatt hours"),
    unit("kW", "kilowatt", "kilowatts"),
    unit("hPa", "hectopascal", "hectopascals"),
    unit("mbar", "millibar", "millibars"),
    unit("km", "kilometre", "kilometres"),
    unit("cm", "centimetre", "centimetres"),
    unit("mm", "millimetre", "millimetres"),
    unit("kg", "kilogram", "kilograms"),
    unit("ms", "millisecond", "milliseconds"),
    unit("%", "percent", "percent"),
];

/// A currency symbol written before an amount, and how to say the currency.
struct Currency {
    symbol: char,
    one: &'static str,
    many: &'static str,
}

/// Rand first: replies quote prices in rand. Rand and cent have no plural "s" in South African
/// English.
const CURRENCIES: &[Currency] = &[
    Currency {
        symbol: 'R',
        one: "rand",
        many: "rand",
    },
    Currency {
        symbol: '$',
        one: "dollar",
        many: "dollars",
    },
    Currency {
        symbol: '€',
        one: "euro",
        many: "euros",
    },
    Currency {
        symbol: '£',
        one: "pound",
        many: "pounds",
    },
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `text` with units, amounts, numbers marked `#`, month abbreviations and ISO dates next to a
/// number written as words.
pub fn speakable(text: &str) -> String {
    let text = months(&iso_dates(text));
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text.as_str();
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let (mut before, from_number) = rest.split_at(start);
        let currency = currency_before(&mut before);
        out.push_str(before);
        if currency.is_none() && out.ends_with('#') {
            out.pop();
            out.push_str("number ");
        }
        let end = from_number
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
            .unwrap_or(from_number.len());
        // A trailing full stop or comma ends the sentence or clause, not the number.
        let number = from_number[..end].trim_end_matches(['.', ',']);
        rest = &from_number[number.len()..];
        if let Some(currency) = currency {
            // "R2D2" is a name, not an amount.
            if rest.starts_with(char::is_alphanumeric) {
                out.push(currency.symbol);
            } else {
                out.push_str(&amount(number, currency));
                continue;
            }
        }
        out.push_str(number);
        let after_space = rest.strip_prefix(' ').unwrap_or(rest);
        if let Some((unit, tail)) = unit_at(after_space) {
            out.push(' ');
            out.push_str(if number == "1" { unit.one } else { unit.many });
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// The currency whose symbol ends `before`, removing it. The symbol must start a word, so the R
/// in "GR125" is left alone.
fn currency_before(before: &mut &str) -> Option<&'static Currency> {
    let currency = CURRENCIES.iter().find(|c| before.ends_with(c.symbol))?;
    let rest = &before[..before.len() - currency.symbol.len_utf8()];
    if rest.chars().next_back().is_some_and(char::is_alphanumeric) {
        return None;
    }
    *before = rest;
    Some(currency)
}

/// `number` as an amount of `currency`: "125 rand", "1 dollar 50 cents". Zero cents are left out.
fn amount(number: &str, currency: &Currency) -> String {
    let (whole, cents) = number.split_once('.').unwrap_or((number, ""));
    let whole = if whole.is_empty() { "0" } else { whole };
    let mut spoken = format!(
        "{whole} {}",
        if whole == "1" {
            currency.one
        } else {
            currency.many
        }
    );
    let cents = match cents.len() {
        0 => 0,
        1 => cents.parse::<u32>().unwrap_or(0) * 10,
        _ => cents.get(..2).and_then(|c| c.parse().ok()).unwrap_or(0),
    };
    if cents > 0 {
        let unit = if cents == 1 { "cent" } else { "cents" };
        spoken.push_str(&format!(" {cents} {unit}"));
    }
    spoken
}

/// `text` with each `YYYY-MM-DD` date written as "26 October 2026".
fn iso_dates(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 16);
    let mut copied = 0;
    let mut i = 0;
    while i + 10 <= bytes.len() {
        let candidate = &bytes[i..i + 10];
        let shape = candidate.iter().enumerate().all(|(n, b)| {
            if n == 4 || n == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
        let bounded = (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            && bytes.get(i + 10).is_none_or(|b| !b.is_ascii_alphanumeric());
        let date = (shape && bounded)
            .then(|| {
                let field = |range: std::ops::Range<usize>| {
                    text[i + range.start..i + range.end].parse::<usize>().ok()
                };
                Some((field(0..4)?, field(5..7)?, field(8..10)?))
            })
            .flatten()
            .filter(|(_, month, day)| (1..=12).contains(month) && (1..=31).contains(day));
        if let Some((year, month, day)) = date {
            out.push_str(&text[copied..i]);
            out.push_str(&format!("{day} {} {year}", MONTHS[month - 1]));
            i += 10;
            copied = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` with each month abbreviation ("Oct", "Sept.") that is next to a number written in full.
fn months(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_ascii_alphabetic()) {
        let (before, word_on) = rest.split_at(start);
        out.push_str(before);
        let len = word_on
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(word_on.len());
        let (word, after) = word_on.split_at(len);
        let month = (!out.ends_with(|c: char| c.is_alphanumeric()))
            .then(|| month(word))
            .flatten();
        let after_dot = after.strip_prefix('.').unwrap_or(after);
        let number_next = after_dot
            .trim_start_matches(' ')
            .starts_with(|c: char| c.is_ascii_digit())
            || out
                .trim_end_matches(' ')
                .ends_with(|c: char| c.is_ascii_digit());
        match month {
            Some(month) if number_next => {
                out.push_str(month);
                // Keep a full stop that ends the sentence; drop the abbreviation's.
                let next = after_dot.trim_start_matches(' ');
                let ends_sentence = after.starts_with('.')
                    && (next.is_empty()
                        || next.starts_with(|c: char| c.is_uppercase() || c == '\n'));
                rest = if !ends_sentence { after_dot } else { after };
            }
            _ => {
                out.push_str(word);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The month `word` abbreviates, as in "Oct" or "Sept". Full names and "May" stay as they are.
fn month(word: &str) -> Option<&'static str> {
    if !(3..=4).contains(&word.len()) {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    let index = match lower.as_str() {
        "jan" => 0,
        "feb" => 1,
        "mar" => 2,
        "apr" => 3,
        "jun" => 5,
        "jul" => 6,
        "aug" => 7,
        "sep" | "sept" => 8,
        "oct" => 9,
        "nov" => 10,
        "dec" => 11,
        _ => return None,
    };
    Some(MONTHS[index])
}

/// The unit `text` starts with, ending at a word boundary, and the text after it.
fn unit_at(text: &str) -> Option<(&'static Unit, &str)> {
    UNITS.iter().find_map(|unit| {
        let tail = text.strip_prefix(unit.written)?;
        let boundary = tail.chars().next().is_none_or(|c| !c.is_alphanumeric());
        boundary.then_some((unit, tail))
    })
}

#[cfg(test)]
mod tests {
    use super::speakable;

    #[test]
    fn weather_units_are_spoken_as_words() {
        assert_eq!(
            speakable("Temperature: 26.6°C, wind 9.1 km/h, gusts 22.3km/h."),
            "Temperature: 26.6 degrees, wind 9.1 kilometres per hour, gusts 22.3 kilometres per hour."
        );
        assert_eq!(
            speakable("Humidity 42%, rain 0.0 mm, pressure 1013 hPa, 70 °F."),
            "Humidity 42 percent, rain 0.0 millimetres, pressure 1013 hectopascals, 70 degrees \
             Fahrenheit."
        );
        assert_eq!(speakable("It is 39 °C."), "It is 39 degrees.");
    }

    #[test]
    fn one_is_singular() {
        assert_eq!(
            speakable("1 km/h and 1°C"),
            "1 kilometre per hour and 1 degree"
        );
    }

    #[test]
    fn amounts_are_spoken_with_the_currency() {
        assert_eq!(
            speakable("It costs R125, or R1,234.50 for the case."),
            "It costs 125 rand, or 1,234 rand 50 cents for the case."
        );
        assert_eq!(
            speakable("R1 each, $2.5 or €3.00."),
            "1 rand each, 2 dollars 50 cents or 3 euros."
        );
        assert_eq!(speakable("£1.01"), "1 pound 1 cent");
        assert_eq!(speakable("Model GR125 and R2D2."), "Model GR125 and R2D2.");
    }

    #[test]
    fn hash_before_a_number_is_number() {
        assert_eq!(
            speakable("Order #2241 arrived."),
            "Order number 2241 arrived."
        );
        assert_eq!(speakable("Use #tags and # 5."), "Use #tags and # 5.");
    }

    #[test]
    fn month_abbreviations_next_to_numbers_are_spoken() {
        assert_eq!(speakable("Delivered Oct 26."), "Delivered October 26.");
        assert_eq!(
            speakable("From 3 Sept. to Nov. 4, 2026"),
            "From 3 September to November 4, 2026"
        );
        assert_eq!(speakable("It ends Dec."), "It ends Dec.");
        assert_eq!(
            speakable("Mar the 5 tables? Jan said."),
            "Mar the 5 tables? Jan said."
        );
    }

    #[test]
    fn iso_dates_are_spoken() {
        assert_eq!(
            speakable("Ordered 2026-10-26, received 2026-11-01."),
            "Ordered 26 October 2026, received 1 November 2026."
        );
        assert_eq!(
            speakable("Ref 12026-10-26 and 2026-13-01"),
            "Ref 12026-10-26 and 2026-13-01"
        );
    }

    #[test]
    fn words_and_numbers_without_units_are_untouched() {
        for text in [
            "Hi Pieter. The largest contentful paint is 154 milliseconds.",
            "Order 12 cases at 2,268 rand.",
            "Take the M5 to km markers. 5 kmart stores, 3 miles.",
            "Version 2.0.1 is out.",
        ] {
            assert_eq!(speakable(text), text);
        }
    }
}

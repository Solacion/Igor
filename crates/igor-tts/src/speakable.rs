//! Rewrites units written for reading into words for speaking.
//!
//! espeak-ng reads `9.1 km/h` as "nine point one k m slash h" and `26.6°C` as "twenty-six point
//! six degrees C". [`speakable`] turns a unit that follows a number into words: "9.1 kilometres
//! per hour", "26.6 degrees". Units are only rewritten after a number, so ordinary words are left
//! alone.

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

/// `text` with each unit that follows a number written as words.
pub fn speakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let (before, from_number) = rest.split_at(start);
        out.push_str(before);
        let end = from_number
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
            .unwrap_or(from_number.len());
        // A trailing full stop or comma ends the sentence or clause, not the number.
        let number = from_number[..end].trim_end_matches(['.', ',']);
        out.push_str(number);
        rest = &from_number[number.len()..];
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

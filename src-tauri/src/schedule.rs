//! Подбор по расписанию: раз в сутки в заданное время (по умолчанию 03:00).
//!
//! Если компьютер в это время спал, подбор догоняет пропуск, но только в пределах
//! [`WINDOW`] — иначе он внезапно начался бы посреди рабочего дня.

/// Сколько минут после назначенного времени ещё можно догнать пропущенный подбор.
pub const WINDOW: i64 = 6 * 60;

/// Местное время: номер дня (от 1970-01-01) и минуты от полуночи.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub day: i64,
    pub minutes: i64,
}

#[cfg(windows)]
pub fn local_now() -> Local {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    Local {
        day: days_from_civil(t.wYear as i64, t.wMonth as i64, t.wDay as i64),
        minutes: t.wHour as i64 * 60 + t.wMinute as i64,
    }
}

#[cfg(not(windows))]
pub fn local_now() -> Local {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    Local { day: s / 86400, minutes: s % 86400 / 60 }
}

/// Номер дня по григорианской дате (алгоритм Говарда Хиннанта).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// «03:00» → минуты от полуночи.
pub fn parse_hm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 60 + m)
}

/// Пора ли запускать: возвращает день, к которому относится запуск,
/// если с назначенного времени прошло меньше [`WINDOW`] и в этот день ещё не запускали.
pub fn due(now: Local, target: i64, last: Option<i64>) -> Option<i64> {
    // назначенное время уже наступило сегодня — или это догоняем вчерашнее (окно через полночь)
    let (day, elapsed) = if now.minutes >= target {
        (now.day, now.minutes - target)
    } else {
        (now.day - 1, now.minutes + 24 * 60 - target)
    };
    (elapsed < WINDOW && last != Some(day)).then_some(day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(days_from_civil(2026, 10, 18) - days_from_civil(2026, 10, 17), 1);
    }

    #[test]
    fn parses_time() {
        assert_eq!(parse_hm("03:00"), Some(180));
        assert_eq!(parse_hm("23:59"), Some(1439));
        assert_eq!(parse_hm("24:00"), None);
        assert_eq!(parse_hm("abc"), None);
    }

    #[test]
    fn due_logic() {
        let at = |day: i64, h: i64, m: i64| Local { day, minutes: h * 60 + m };
        // до назначенного времени — рано (вчерашнее окно давно прошло)
        assert_eq!(due(at(10, 2, 59), 180, None), None);
        // ровно в срок и чуть позже
        assert_eq!(due(at(10, 3, 0), 180, None), Some(10));
        assert_eq!(due(at(10, 8, 59), 180, None), Some(10));
        // окно закрылось
        assert_eq!(due(at(10, 9, 0), 180, None), None);
        // уже запускали сегодня
        assert_eq!(due(at(10, 4, 0), 180, Some(10)), None);
        // окно через полночь: назначено на 23:00, сейчас 01:00 следующего дня
        assert_eq!(due(at(11, 1, 0), 23 * 60, Some(9)), Some(10));
        assert_eq!(due(at(11, 1, 0), 23 * 60, Some(10)), None);
    }
}

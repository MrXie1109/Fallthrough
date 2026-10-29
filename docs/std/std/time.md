# `std.time`

Time: durations, monotonic instants, and the wall clock.

Three questions, three types:

* `Duration` is an amount of time, in nanoseconds.
* `Instant` is a point on the monotonic clock, which only moves forward --
  for measuring how long something took.
* `SystemTime` is a point on the wall clock, in Unix epoch nanoseconds --
  for saying *when* something happened.

    start Instant = time.now();
    ... work ...
    took Duration = time.since(&start);
    std.print("took {} ms\n", took.millis())??;

# Why two clocks

The wall clock can be changed by a person or a time server, so a duration
measured with it can be negative or wrong. The monotonic clock cannot, which
is why `Instant` is the type for measuring and `SystemTime` for recording.

## Declarations

### `export type Duration {`

An amount of time.

The unit is the nanosecond, the finest resolution the platforms here report;
`millis()` and `seconds()` truncate.

```
export type Duration {
    // The amount, in nanoseconds.
    _ns i64;
}
```

### `export fn __init__(this &Duration, nanos i64) void {`

A duration of `nanos` nanoseconds.

```
export fn __init__(this &Duration, nanos i64) void {
    this._ns = nanos;
}
```

### `export fn from_nanos(nanos i64) Duration {`

A duration of `nanos` nanoseconds.

```
export fn from_nanos(nanos i64) Duration {
    return Duration(nanos);
}
```

### `export fn from_millis(millis i64) Duration {`

A duration of `millis` milliseconds.

```
export fn from_millis(millis i64) Duration {
    return Duration(millis * 1000000i64);
}
```

### `export fn from_seconds(seconds i64) Duration {`

A duration of `seconds` seconds.

```
export fn from_seconds(seconds i64) Duration {
    return Duration(seconds * 1000000000i64);
}
```

### `export fn nanos(this &const Duration) i64 {`

The duration in whole nanoseconds.

```
export fn nanos(this &const Duration) i64 {
    return this._ns;
}
```

### `export fn millis(this &const Duration) i64 {`

The duration in whole milliseconds, truncated toward zero.

```
export fn millis(this &const Duration) i64 {
    return this._ns / 1000000i64;
}
```

### `export fn seconds(this &const Duration) i64 {`

The duration in whole seconds, truncated toward zero.

```
export fn seconds(this &const Duration) i64 {
    return this._ns / 1000000000i64;
}
```

### `export fn __add__(this &const Duration, other &const Duration) Duration {`

The sum of two durations.

```
export fn __add__(this &const Duration, other &const Duration) Duration {
    return Duration(this._ns + other._ns);
}
```

### `export fn __sub__(this &const Duration, other &const Duration) Duration {`

The difference of two durations.

```
export fn __sub__(this &const Duration, other &const Duration) Duration {
    return Duration(this._ns - other._ns);
}
```

### `export fn __eq__(this &const Duration, other &const Duration) bool {`

Whether two durations are the same amount.

```
export fn __eq__(this &const Duration, other &const Duration) bool {
    return this._ns == other._ns;
}
```

### `export fn __ne__(this &const Duration, other &const Duration) bool {`

Whether two durations differ.

```
export fn __ne__(this &const Duration, other &const Duration) bool {
    return this._ns != other._ns;
}
```

### `export fn __lt__(this &const Duration, other &const Duration) bool {`

Whether this duration is shorter than another.

```
export fn __lt__(this &const Duration, other &const Duration) bool {
    return this._ns < other._ns;
}
```

### `export fn __str__(this &const Duration) String {`

The duration as text: seconds with three decimals, as a terminal would write
it.

```
export fn __str__(this &const Duration) String {
    negative bool = this._ns < 0i64;
    nanos i64 = this._ns;
    if (negative) {
        nanos = 0i64 - nanos;
    }
    whole i64 = nanos / 1000000000i64;
    fraction i64 = (nanos % 1000000000i64) / 1000000i64;
    text String = String();
    if (negative) {
        text.push('-');
    }
    print_to(text, whole as int);
    text.push('.');
    if (fraction < 100i64) {
        text.push('0');
    }
    if (fraction < 10i64) {
        text.push('0');
    }
    print_to(text, fraction as int);
    text.push('s');
    return text;
}
```

### `fn print_to(text &String, value int) void`

The number as decimal digits appended to `text`.

A small helper rather than a `String` conversion: `str` of an `int` builds a
`String` of its own, and the formatting above needs the digits placed between
parts of a line it is already building.

- `text`: the line being built
- `value`: the number to append, with a leading `-` when negative

```
fn print_to(text &String, value int) void
```

### `export type Instant {`

A point on the monotonic clock.

Only differences of two instants mean anything; the number inside is the
platform's own count of nanoseconds since some arbitrary start.

```
export type Instant {
    // Nanoseconds on the monotonic clock.
    _ns i64;
}
```

### `export fn __init__(this &Instant, nanos i64) void {`

An instant from a monotonic count.

```
export fn __init__(this &Instant, nanos i64) void {
    this._ns = nanos;
}
```

### `export fn now() Instant {`

The current instant of the monotonic clock.

```
export fn now() Instant {
    return Instant(os.monotonic_ns() as i64);
}
```

### `export fn since(start &const Instant) Duration {`

How long has passed since `start`.

```
export fn since(start &const Instant) Duration {
    return Duration((os.monotonic_ns() as i64) - start._ns);
}
```

### `export type SystemTime {`

A point on the wall clock.

The value is nanoseconds since the Unix epoch (1970-01-01T00:00:00Z), which
is the representation a file time and a log line share.

```
export type SystemTime {
    // Nanoseconds since the Unix epoch.
    _ns i64;
}
```

### `export fn __init__(this &SystemTime, nanos i64) void {`

A wall-clock time from Unix-epoch nanoseconds.

```
export fn __init__(this &SystemTime, nanos i64) void {
    this._ns = nanos;
}
```

### `export fn unix_now() SystemTime {`

The current instant of the wall clock.

```
export fn unix_now() SystemTime {
    return SystemTime(time_sys.realtime_ns());
}
```

### `export fn unix_nanos() i64 {`

The current wall-clock time as nanoseconds since the Unix epoch.

```
export fn unix_nanos() i64 {
    return time_sys.realtime_ns();
}
```

### `export fn unix_seconds(this &const SystemTime) i64 {`

Whole seconds since the Unix epoch.

```
export fn unix_seconds(this &const SystemTime) i64 {
    return this._ns / 1000000000i64;
}
```

### `export fn __as__(this &const SystemTime, target *DateTime) DateTime {`

The civil date and time in UTC.

```
export fn __as__(this &const SystemTime, target *DateTime) DateTime {
    seconds i64 = this._ns / 1000000000i64;
    days i64 = seconds / 86400i64;
    remainder i64 = seconds % 86400i64;
    if (remainder < 0i64) {
        days -= 1i64;
        remainder += 86400i64;
    }
    hour i64 = remainder / 3600i64;
    minute i64 = (remainder % 3600i64) / 60i64;
    second i64 = remainder % 60i64;
    civil Civil = civil_from_days(days);
    return DateTime(civil.year, civil.month, civil.day, hour as uint, minute as uint, second as uint);
}
```

### `type Civil {`

The civil year, month and day of a count of days since the Unix epoch.

```
type Civil {
    year int;
    month uint;
    day uint;
}
```

### `fn civil_from_days(days i64) Civil`

Converts a day count to a civil date.

Howard Hinnant's `civil_from_days`: shift the epoch to 0000-03-01, which makes
the leap day the last day of the year, then divide by the 400-year cycle. The
arithmetic is integer only, and correct for every date the epoch can express.

- `days`: days since the Unix epoch, which may be negative

```
fn civil_from_days(days i64) Civil
```

### `export type DateTime {`

A civil date and time in UTC.

```
export type DateTime {
    // The year, as written.
    _year int;
    // The month, 1 through 12.
    _month uint;
    // The day of the month, 1 through 31.
    _day uint;
    // The hour, 0 through 23.
    _hour uint;
    // The minute, 0 through 59.
    _minute uint;
    // The second, 0 through 59.
    _second uint;
}
```

### `export fn __init__(`

Builds a date and time from its fields.

```
export fn __init__(
    this &DateTime,
    year int,
    month uint,
    day uint,
    hour uint,
    minute uint,
    second uint
) void {
    this._year = year;
    this._month = month;
    this._day = day;
    this._hour = hour;
    this._minute = minute;
    this._second = second;
}
```

### `export fn year(this &const DateTime) int {`

The year.

```
export fn year(this &const DateTime) int {
    return this._year;
}
```

### `export fn month(this &const DateTime) uint {`

The month.

```
export fn month(this &const DateTime) uint {
    return this._month;
}
```

### `export fn day(this &const DateTime) uint {`

The day of the month.

```
export fn day(this &const DateTime) uint {
    return this._day;
}
```

### `export fn hour(this &const DateTime) uint {`

The hour.

```
export fn hour(this &const DateTime) uint {
    return this._hour;
}
```

### `export fn minute(this &const DateTime) uint {`

The minute.

```
export fn minute(this &const DateTime) uint {
    return this._minute;
}
```

### `export fn second(this &const DateTime) uint {`

The second.

```
export fn second(this &const DateTime) uint {
    return this._second;
}
```

### `export fn __str__(this &const DateTime) String {`

The date and time as `YYYY-MM-DD HH:MM:SS`.

```
export fn __str__(this &const DateTime) String {
    text String = String();
    append_padded(&text, this._year as i64, 4);
    text.push('-');
    append_padded(&text, this._month as i64, 2);
    text.push('-');
    append_padded(&text, this._day as i64, 2);
    text.push(' ');
    append_padded(&text, this._hour as i64, 2);
    text.push(':');
    append_padded(&text, this._minute as i64, 2);
    text.push(':');
    append_padded(&text, this._second as i64, 2);
    return text;
}
```

### `fn append_padded(text &String, value i64, width usize) void`

Appends `value` to `text` with at least `width` digits.

- `text`: the line being built
- `value`: the number to append; its sign is written first
- `width`: the fewest digits to write, zero-padded on the left

```
fn append_padded(text &String, value i64, width usize) void
```

### `export fn sleep(duration &const Duration) void {`

Sleeps for `duration`.

```
export fn sleep(duration &const Duration) void {
    millis i64 = duration.millis();
    if (millis <= 0i64) {
        return;
    }
    os.sleep_ms(millis as int);
}
```

# `std.random`

Pseudo-random numbers: a Mersenne Twister, its distributions, and the global
generator a program uses without making one.

# The one call

    n uint = std.random();               // 32 random bits
    die int = std.random.uniform(1, 6); // 1..=6, both ends included

`std.random` is one `Random` value for the whole program. It is seeded at
setup from the system's entropy source, so two runs of the same program draw
different numbers; `std.random.seed(n)` makes a run repeatable, and a program
that wants its own stream makes its own `Random`.

# The generator

MT19937, the Mersenne Twister of the C++ standard library: a 624-word state,
a period of 2^19937 - 1, and the same numbers C++'s `std::mt19937` produces
for the same seed. It is not a cryptographic generator -- no pseudo-random
generator is, except the ones built for it -- and the seeding path here is the
one C++ uses, so a seed and a draw agree across the two libraries.

# The distributions

`uniform` over an integer range, `uniform_real` over a real interval,
`normal`, `lognormal`, `exponential`, `poisson`, `binomial` and `geometric`,
`bernoulli` for a coin with a given bias, `choice` and `weighted_choice` for
picking an element, and `shuffle` for a container in place. Each takes the
generator, so the same `Random` drives all of them.

## Declarations

### `STATE_WORDS const usize = 624;`

The words of MT19937's state, and the index of the next one to use.

```
STATE_WORDS const usize = 624;
```

### `SHIFT_OFFSET const usize = 397;`

The period parameter.

```
SHIFT_OFFSET const usize = 397;
```

### `MATRIX_A const uint = 0x9908b0dfu32;`

The bit that makes the twisted value a member of the field.

```
MATRIX_A const uint = 0x9908b0dfu32;
```

### `UPPER_MASK const uint = 0x80000000u32;`

The high and low halves of two consecutive words, split for the twist.

```
UPPER_MASK const uint = 0x80000000u32;
LOWER_MASK const uint = 0x7fffffffu32;
```

### `DEFAULT_SEED const uint = 5489;`

The default seed, the one C++'s `std::mt19937` is constructed with.

```
DEFAULT_SEED const uint = 5489;
```

### `export type Random {`

A Mersenne Twister generator.

The state is the whole object: copying a `Random` copies its stream, which is
what a function that takes one by value would do with any other value.

```
export type Random {
    _state [624]uint;
    _index usize;
}
```

### `export fn __init__(this &Random) void {`

Creates a generator with the default seed.

```
export fn __init__(this &Random) void {
    this.seed(DEFAULT_SEED);
}
```

### `export fn __init__(this &Random, seed uint) void {`

Creates a generator from a seed.

```
export fn __init__(this &Random, seed uint) void {
    this.seed(seed);
}
```

### `fn _generate(this &Random) void`

Turns the state `index` words to the next 624, and resets the index.

```
fn _generate(this &Random) void
```

### `fn next_u32(this &Random) uint`

Returns the next 32 random bits.

This is the generator itself: one draw, one twist when the state runs out,
and the standard's tempering so that the bits are equally likely in every
position. The public spelling is `random()`; the word count is not part of
the surface a program decides about.

```
fn next_u32(this &Random) uint
```

### `export fn __call__(this &Random) uint {`

Returns the next 32 random bits.

`std.random()` is one draw; it is the generator's one public way to produce a
word, and every distribution below is built on it.

```
export fn __call__(this &Random) uint {
    return this.next_u32();
}
```

### `export fn seed(this &Random, value uint) void {`

Seeds the generator.

The state is filled the way MT19937's reference implementation fills it, so
the same seed produces the same stream as C++'s `std::mt19937`.

```
export fn seed(this &Random, value uint) void {
    this._state[0] = value;
    index usize = 1;
    while (index < STATE_WORDS) {
        previous uint = this._state[index - 1];
        mixed uint = previous ^ (previous >> 30);
        this._state[index] = ((mixed as u64) * 1812433253u64 + (index as u64)) as uint;
        index += 1;
    }
    this._index = STATE_WORDS;
}
```

### `export fn seed_from_entropy(this &Random) bool except {`

Seeds the generator from the system's entropy source.

Returns `false` when the platform could not supply the bytes; the caller then
decides what to do, and the setup below seeds from the clock instead of
leaving every run identical.

```
export fn seed_from_entropy(this &Random) bool except {
    bytes [2496]byte;
    if (!random_entropy.fill(&bytes[0], 2496)?? ) {
        return false;
    }
    index usize = 0;
    while (index < STATE_WORDS) {
        word uint = 0;
        byte_index usize = 0;
        while (byte_index < 4) {
            word = word | ((bytes[index * 4 + byte_index] as uint) << ((byte_index * 8) as uint));
            byte_index += 1;
        }
        this._state[index] = word;
        index += 1;
    }
    this._index = STATE_WORDS;
    return true;
}
```

### `fn uniform(this &Random, low T, high T) T`

Returns a value in `low..=high`, both ends included.

The draw is rejected when it would fall in the incomplete last block, which is
what keeps every value equally likely: taking a remainder directly would make
the low values slightly more common.
Draws a number in `[low, high]`, included at both ends.

The argument type chooses the distribution: an integer type takes the integer
form (rejection sampling, no modulo bias), a floating type draws in
`[low, high)`. The compiler's `IsFloating` trait decides at compile time, so
there is one call to remember and no `_real` suffix.

- `low`: the smallest value the draw may give
- `high`: the largest, included

```
export
template <T type>
fn uniform(this &Random, low T, high T) T
```

### `fn uniform_int(this &Random, low int, high int) int`

Draws an integer in `[low, high]`, included at both ends, by rejection.

- `low`: the smallest value the draw may give
- `high`: the largest, included; a `high` at or below `low` gives `low`

```
fn uniform_int(this &Random, low int, high int) int
```

### `export fn uniform_real(this &Random, low f64, high f64) f64 {`

Returns a real in `[low, high)`.

A generator word is divided by 2^32, so the result is in `[0, 1)` with the
resolution of one word.

```
export fn uniform_real(this &Random, low f64, high f64) f64 {
    fraction f64 = (this.next_u32() as f64) / 4294967296.0;
    return low + fraction * (high - low);
}
```

### `export fn normal(this &Random, mean f64, deviation f64) f64 {`

Returns a real from the normal distribution with `mean` and `deviation`.

Box-Muller: two independent uniforms become two independent normals through
a radius and an angle, and one of them is returned. The radius is `log` of the
first, so a draw of zero would make it infinite; zero is drawn away from.

```
export fn normal(this &Random, mean f64, deviation f64) f64 {
    u1 f64 = this.uniform_real(0.0, 1.0);
    if (u1 <= 0.0) {
        u1 = 1.0 / 4294967296.0;
    }
    u2 f64 = this.uniform_real(0.0, 1.0);
    radius f64 = C.sqrt(-2.0 * C.log(u1));
    angle f64 = 6.283185307179586 * u2;
    return mean + deviation * radius * C.cos(angle);
}
```

### `export fn bernoulli(this &Random, chance f64) bool {`

Returns `true` with probability `chance`, which must be in `[0, 1]`.

```
export fn bernoulli(this &Random, chance f64) bool {
    return this.uniform_real(0.0, 1.0) < chance;
}
```

### `export fn exponential(this &Random, rate f64) f64 {`

Returns a real from the exponential distribution with the given `rate`.

The inverse of the cumulative distribution: `-log(1 - u) / rate`, with `u`
drawn away from zero so the logarithm is finite.

```
export fn exponential(this &Random, rate f64) f64 {
    if (rate <= 0.0) {
        util.panic("exponential needs a positive rate");
    }
    u f64 = this.uniform_real(0.0, 1.0);
    if (u <= 0.0) {
        u = 1.0 / 4294967296.0;
    }
    return -C.log(1.0 - u) / rate;
}
```

### `export fn lognormal(this &Random, mean f64, deviation f64) f64 {`

Returns a real from the log-normal distribution: `exp` of a normal draw.

```
export fn lognormal(this &Random, mean f64, deviation f64) f64 {
    return C.exp(this.normal(mean, deviation));
}
```

### `export fn poisson(this &Random, mean f64) uint {`

Returns a count from the Poisson distribution with the given `mean`.

Knuth's method for a small mean: multiply uniforms until the product falls
below `exp(-mean)`, and the number of factors is the count. A large mean takes
the normal approximation, which keeps the loop bounded; the mean and variance
of both are `mean`.

```
export fn poisson(this &Random, mean f64) uint {
    if (mean < 0.0) {
        util.panic("poisson needs a non-negative mean");
    }
    if (mean > 30.0) {
        draw f64 = this.normal(mean, C.sqrt(mean));
        if (draw < 0.0) {
            return 0u32;
        }
        return C.round(draw) as uint;
    }
    limit f64 = C.exp(0.0 - mean);
    product f64 = 1.0;
    count uint = 0;
    while (product > limit) {
        product = product * this.uniform_real(0.0, 1.0);
        count += 1;
    }
    return count - 1u32;
}
```

### `export fn binomial(this &Random, trials usize, chance f64) usize {`

Returns how many of `trials` independent coins with `chance` came up true.

One Bernoulli draw per trial, so the work is linear in `trials`; the mean is
`trials * chance`.

```
export fn binomial(this &Random, trials usize, chance f64) usize {
    successes usize = 0;
    index usize = 0;
    while (index < trials) {
        if (this.bernoulli(chance)) {
            successes += 1;
        }
        index += 1;
    }
    return successes;
}
```

### `export fn geometric(this &Random, chance f64) uint {`

Returns the number of trials up to and including the first success.

The support is `1, 2, 3, ...`, and the mean is `1 / chance`. `chance` must be
in `(0, 1]`.

```
export fn geometric(this &Random, chance f64) uint {
    if (chance <= 0.0 || chance > 1.0) {
        util.panic("geometric needs a chance in (0, 1]");
    }
    if (chance >= 1.0) {
        return 1u32;
    }
    u f64 = this.uniform_real(0.0, 1.0);
    if (u <= 0.0) {
        u = 1.0 / 4294967296.0;
    }
    return (C.log(1.0 - u) / C.log(1.0 - chance)) as uint + 1u32;
}
```

### `fn choice(this &Random, values &const Array<T>) &const T`

Picks one element of `values`, every element equally likely.

- `values`: the elements to pick from; it must not be empty

```
export
template <T type>
fn choice(this &Random, values &const Array<T>) &const T
```

### `fn weighted_choice(`

Picks one element of `values`, with probability proportional to `weights`.

The weights are relative, not a distribution: `{1, 3}` means the second
element is three times as likely. Both arrays must be the same length, and at
least one weight must be positive.

- `values`: the elements to pick from
- `weights`: how likely each element is, relative to the others

```
export
template <T type>
fn weighted_choice(
    this &Random,
    values &const Array<T>,
    weights &const Array<f64>
) &const T
```

### `fn shuffle(this &Random, values *Array<T>) void`

Shuffles an array in place, every order equally likely.

The array is taken by pointer because the shuffle writes into the caller's
elements: `std.random.shuffle(&values)`.

Fisher-Yates: for each position from the end, swap it with a position drawn
from the prefix that ends there.

- `values`: the sequence to shuffle in place

```
export
template <T type>
fn shuffle(this &Random, values *Array<T>) void
```

### `export random Random;`

The generator the whole program shares.

It is seeded at setup from the system's entropy source when one is available.
A run that must be repeatable calls `std.random.seed(..)` before drawing, and
a library that must not disturb the program's stream makes its own `Random`.

```
export random Random;
```

### `fn __setup__() void`

```
fn __setup__() void
```

# `std.math`

Numbers: the operations every numeric type needs, and the ones that work on a
real number.

`min`, `max`, `clamp`, `abs` and `square` ask only what a number already
answers, so they are templates and cost nothing at run time. Square roots,
powers, floors and trigonometry are the C library's, on `f64`.

## Declarations

### `fn min(a T, b T) T`

Returns the smaller of `a` and `b`.

- `a`: one of the two numbers
- `b`: the other

```
export
template <T type>
fn min(a T, b T) T
```

### `fn max(a T, b T) T`

Returns the larger of `a` and `b`.

- `a`: one of the two numbers
- `b`: the other

```
export
template <T type>
fn max(a T, b T) T
```

### `fn clamp(value T, low T, high T) T`

Returns `value` brought into `[low, high]`.

- `value`: the number to bring in
- `low`: the range's lower bound
- `high`: the range's upper bound

```
export
template <T type>
fn clamp(value T, low T, high T) T
```

### `fn abs(value T) T`

Returns the value without its sign.

An unsigned type has no sign to remove, and `value < 0` is false for it, so the
same body is right for both.

- `value`: the number whose magnitude is wanted

```
export
template <T type>
fn abs(value T) T
```

### `fn square(value T) T`

Returns the value multiplied by itself.

- `value`: the number to multiply by itself

```
export
template <T type>
fn square(value T) T
```

### `fn squared_length(a T, b T) T`

Returns the sum of the squares, which is what a length is built from.

- `a`: one coordinate
- `b`: the other coordinate

```
export
template <T type>
fn squared_length(a T, b T) T
```

### `export fn sqrt(value f64) f64 {`

Returns the square root.

- `value`: the number whose square root is wanted

```
export fn sqrt(value f64) f64 {
    return C.sqrt(value);
}
```

### `export fn pow(base f64, exponent f64) f64 {`

Raises `base` to `exponent`.

```
export fn pow(base f64, exponent f64) f64 {
    return C.pow(base, exponent);
}
```

### `export fn floor(value f64) f64 {`

Returns the largest integer that is not greater than `value`.

- `value`: the number to round down

```
export fn floor(value f64) f64 {
    return C.floor(value);
}
```

### `export fn ceil(value f64) f64 {`

Returns the smallest integer that is not less than `value`.

- `value`: the number to round up

```
export fn ceil(value f64) f64 {
    return C.ceil(value);
}
```

### `export fn round(value f64) f64 {`

Returns the nearest integer, with halves going away from zero.

- `value`: the number to round to the nearest whole

```
export fn round(value f64) f64 {
    return C.round(value);
}
```

### `export fn fabs(value f64) f64 {`

Returns the absolute value of a real number.

- `value`: the number whose absolute value is wanted

```
export fn fabs(value f64) f64 {
    return C.fabs(value);
}
```

### `export fn exp(power f64) f64 {`

Returns `e` raised to `power`.

- `power`: the exponent to raise `e` to

```
export fn exp(power f64) f64 {
    return C.exp(power);
}
```

### `export fn log(value f64) f64 {`

Returns the natural logarithm.

- `value`: the number whose natural logarithm is wanted

```
export fn log(value f64) f64 {
    return C.log(value);
}
```

### `export fn sin(angle f64) f64 {`

Returns the sine of an angle in radians.

- `angle`: the angle in radians

```
export fn sin(angle f64) f64 {
    return C.sin(angle);
}
```

### `export fn cos(angle f64) f64 {`

Returns the cosine of an angle in radians.

- `angle`: the angle in radians

```
export fn cos(angle f64) f64 {
    return C.cos(angle);
}
```

### `export fn tan(angle f64) f64 {`

Returns the tangent of an angle in radians.

- `angle`: the angle in radians

```
export fn tan(angle f64) f64 {
    return C.tan(angle);
}
```

### `export fn asin(value f64) f64 {`

Returns the angle whose sine is `value`, in radians.

- `value`: the value whose arcsine is wanted, in `[-1, 1]`

```
export fn asin(value f64) f64 {
    return C.asin(value);
}
```

### `export fn acos(value f64) f64 {`

Returns the angle whose cosine is `value`, in radians.

- `value`: the value whose arccosine is wanted, in `[-1, 1]`

```
export fn acos(value f64) f64 {
    return C.acos(value);
}
```

### `export fn atan(value f64) f64 {`

Returns the angle whose tangent is `value`, in radians.

- `value`: the value whose arctangent is wanted

```
export fn atan(value f64) f64 {
    return C.atan(value);
}
```

### `export fn atan2(y f64, x f64) f64 {`

Returns the angle of the point `(x, y)`, in radians.

```
export fn atan2(y f64, x f64) f64 {
    return C.atan2(y, x);
}
```

### `export fn pi() f64 {`

------------------------------------------------------------------ constants

Real constants are functions rather than module constants: a module constant
has to be a constant expression, and a real literal is not one. `pi()` reads
the same in a formula and costs nothing after inlining.
The value the C library calls `M_PI`; named here so a formula reads the same
on every platform.

```
export fn pi() f64 {
    return 3.141592653589793;
}
// The value the C library calls `M_E`.
export fn e() f64 {
    return 2.718281828459045;
}
```

### `export fn tau() f64 {`

A full turn in radians, `2 * pi()`.

```
export fn tau() f64 {
    return 6.283185307179586;
}
```

### `export fn infinity() f64 {`

Positive infinity.

Through `pow` rather than a literal division: the compiler folds a constant
`1.0 / 0.0` before the IEEE rules apply, and this has to be infinity at run
time.

```
export fn infinity() f64 {
    return C.pow(10.0, 400.0);
}
```

### `export fn nan() f64 {`

Not a number.

The square root of a negative value is the one operation IEEE says is NaN and
that the C library computes rather than the compiler folding.

```
export fn nan() f64 {
    return C.sqrt(-1.0);
}
```

### `export fn hypot(x f64, y f64) f64 {`

------------------------------------------------------------------ real functions
The length of the hypotenuse `sqrt(x*x + y*y)` without the intermediate
overflow a naive square would cause.

```
export fn hypot(x f64, y f64) f64 {
    return C.hypot(x, y);
}
```

### `export fn cbrt(value f64) f64 {`

The cube root, which is defined for negative values too.

- `value`: the number whose cube root is wanted

```
export fn cbrt(value f64) f64 {
    return C.cbrt(value);
}
```

### `export fn trunc(value f64) f64 {`

The integer part, toward zero.

```
export fn trunc(value f64) f64 {
    return C.trunc(value);
}
```

### `export fn fract(value f64) f64 {`

The fractional part, with the sign of `value`.

```
export fn fract(value f64) f64 {
    return value - C.trunc(value);
}
```

### `export fn sign(value f64) f64 {`

The sign of `value`: -1, 0 or 1.

```
export fn sign(value f64) f64 {
    if (value > 0.0) {
        return 1.0;
    }
    if (value < 0.0) {
        return -1.0;
    }
    return 0.0;
}
```

### `export fn signum(value f64) int {`

`sign` as an integer, for code that branches on the direction.

```
export fn signum(value f64) int {
    if (value > 0.0) {
        return 1;
    }
    if (value < 0.0) {
        return -1;
    }
    return 0;
}
```

### `export fn lerp(low f64, high f64, fraction f64) f64 {`

Linear interpolation: `low` at `0`, `high` at `1`.

```
export fn lerp(low f64, high f64, fraction f64) f64 {
    return low + (high - low) * fraction;
}
```

### `export fn radians(degrees f64) f64 {`

`degrees` turned into radians.

```
export fn radians(degrees f64) f64 {
    return degrees * (3.141592653589793 / 180.0);
}
```

### `export fn degrees(radians f64) f64 {`

`radians` turned into degrees.

```
export fn degrees(radians f64) f64 {
    return radians * (180.0 / 3.141592653589793);
}
```

### `export fn log2(value f64) f64 {`

The base-2 logarithm.

```
export fn log2(value f64) f64 {
    return C.log2(value);
}
```

### `export fn log10(value f64) f64 {`

The base-10 logarithm.

```
export fn log10(value f64) f64 {
    return C.log10(value);
}
```

### `export fn sinh(value f64) f64 {`

The hyperbolic sine.

```
export fn sinh(value f64) f64 {
    return C.sinh(value);
}
```

### `export fn cosh(value f64) f64 {`

The hyperbolic cosine.

```
export fn cosh(value f64) f64 {
    return C.cosh(value);
}
```

### `export fn tanh(value f64) f64 {`

The hyperbolic tangent.

```
export fn tanh(value f64) f64 {
    return C.tanh(value);
}
```

### `export fn asinh(value f64) f64 {`

The inverse hyperbolic sine.

```
export fn asinh(value f64) f64 {
    return C.asinh(value);
}
```

### `export fn acosh(value f64) f64 {`

The inverse hyperbolic cosine.

```
export fn acosh(value f64) f64 {
    return C.acosh(value);
}
```

### `export fn atanh(value f64) f64 {`

The inverse hyperbolic tangent.

```
export fn atanh(value f64) f64 {
    return C.atanh(value);
}
```

### `export fn is_nan(value f64) bool {`

Whether `value` is NaN.

The C library's `isnan` rather than `value != value`: the comparison is a
constant the optimiser is allowed to fold, and it did, which made every value
look like a number.

```
export fn is_nan(value f64) bool {
    return C.isnan(value) != 0;
}
```

### `export fn is_infinite(value f64) bool {`

Whether `value` is infinite.

```
export fn is_infinite(value f64) bool {
    return C.isinf(value) != 0;
}
```

### `export fn is_finite(value f64) bool {`

Whether `value` is finite: neither NaN nor infinite.

```
export fn is_finite(value f64) bool {
    return !is_nan(value) && !is_infinite(value);
}
```

### `fn is_even(value T) bool`

------------------------------------------------------------------ integers
Whether `value` is even.

- `value`: the integer to test

```
@enable_if(IsInteger(T))
export
template <T type>
fn is_even(value T) bool
```

### `fn is_odd(value T) bool`

Whether `value` is odd.

- `value`: the integer to test

```
@enable_if(IsInteger(T))
export
template <T type>
fn is_odd(value T) bool
```

### `fn gcd(first T, second T) T`

The greatest common divisor of two integers, always non-negative.

Euclid's algorithm, written with the remainder so it never needs a division
that could overflow for the most negative value.

- `first`: one integer
- `second`: the other integer

```
@enable_if(IsInteger(T))
export
template <T type>
fn gcd(first T, second T) T
```

### `fn lcm(first T, second T) T`

The least common multiple of two integers.

- `first`: one integer
- `second`: the other integer

```
@enable_if(IsInteger(T))
export
template <T type>
fn lcm(first T, second T) T
```

### `fn is_power_of_two(value T) bool`

Whether `value` is a power of two (zero is not).

- `value`: the integer to test

```
@enable_if(IsInteger(T))
export
template <T type>
fn is_power_of_two(value T) bool
```

### `fn isqrt(value T) T`

The integer square root: the largest `r` with `r * r <= value`.

- `value`: the number whose root is wanted

```
@enable_if(IsInteger(T))
export
template <T type>
fn isqrt(value T) T
```

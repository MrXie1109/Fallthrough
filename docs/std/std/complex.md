# `std.complex`

Complex numbers, as a value type.

    sum Complex = complex.add(.{ 1.0, 2.0 }, .{ 3.0, -4.0 });
    length f64 = sum.abs();

A complex number is a pair of reals, so it is a two-field type: no allocation,
no ownership, and every operation returns a value. The arithmetic is the
textbook formulas, division included (multiply by the conjugate).

## Declarations

### `export type Complex {`

A complex number `real + imaginary * i`.

```
export type Complex {
    // The real part.
    re f64;
    // The imaginary part.
    im f64;
}
```

### `export fn __init__(this &Complex, real f64, imaginary f64) void {`

Makes a complex number.

```
export fn __init__(this &Complex, real f64, imaginary f64) void {
    this.re = real;
    this.im = imaginary;
}
```

### `export fn i() Complex {`

The imaginary unit.

```
export fn i() Complex {
    return Complex(0.0, 1.0);
}
```

### `export fn add(this &const Complex, second &const Complex) Complex {`

Addition.

```
export fn add(this &const Complex, second &const Complex) Complex {
    return Complex(this.re + second.re, this.im + second.im);
}
```

### `export fn sub(this &const Complex, second &const Complex) Complex {`

Subtraction.

```
export fn sub(this &const Complex, second &const Complex) Complex {
    return Complex(this.re - second.re, this.im - second.im);
}
```

### `export fn mul(this &const Complex, second &const Complex) Complex {`

Multiplication: `(a+bi)(c+di) = (ac-bd) + (ad+bc)i`.

```
export fn mul(this &const Complex, second &const Complex) Complex {
    return Complex(
        this.re * second.re - this.im * second.im,
        this.re * second.im + this.im * second.re
    );
}
```

### `export fn div(this &const Complex, second &const Complex) Complex except {`

Division, by multiplying numerator and denominator by the conjugate.

```
export fn div(this &const Complex, second &const Complex) Complex except {
    denominator f64 = second.re * second.re + second.im * second.im;
    if (denominator == 0.0) {
        raise "ValueError": "division by zero";
    }
    return Complex(
        (this.re * second.re + this.im * second.im) / denominator,
        (this.im * second.re - this.re * second.im) / denominator
    );
}
```

### `export fn conjugate(this &const Complex) Complex {`

The conjugate `a - bi`.

```
export fn conjugate(this &const Complex) Complex {
    return Complex(this.re, -this.im);
}
```

### `export fn abs(this &const Complex) f64 {`

The magnitude.

```
export fn abs(this &const Complex) f64 {
    return math.hypot(this.re, this.im);
}
```

### `export fn arg(this &const Complex) f64 {`

The argument: the angle from the positive real axis, in radians.

```
export fn arg(this &const Complex) f64 {
    return math.atan2(this.im, this.re);
}
```

### `export fn norm_squared(this &const Complex) f64 {`

The square of the magnitude, which avoids a square root.

```
export fn norm_squared(this &const Complex) f64 {
    return this.re * this.re + this.im * this.im;
}
```

### `export fn pow(this &const Complex, exponent f64) Complex except {`

`value` raised to a real power, through polar form.

```
export fn pow(this &const Complex, exponent f64) Complex except {
    magnitude f64 = this.abs();
    if (magnitude == 0.0) {
        if (exponent == 0.0) {
            return Complex(1.0, 0.0);
        }
        return Complex(0.0, 0.0);
    }
    angle f64 = this.arg() * exponent;
    scale f64 = math.pow(magnitude, exponent);
    return Complex(scale * math.cos(angle), scale * math.sin(angle));
}
```

### `export fn __eq__(this &const Complex, other &const Complex) bool {`

Whether two complex numbers are equal.

```
export fn __eq__(this &const Complex, other &const Complex) bool {
    return this.re == other.re && this.im == other.im;
}
```

### `export fn __ne__(this &const Complex, other &const Complex) bool {`

Whether two complex numbers differ.

```
export fn __ne__(this &const Complex, other &const Complex) bool {
    return !(this == other);
}
```

### `export fn __str__(this &const Complex) String {`

`a + bi`, so `{}` prints it.

```
export fn __str__(this &const Complex) String {
    text String = String();
    text.push(str(this.re));
    if (this.im >= 0.0) {
        text.push("+");
    }
    text.push(str(this.im));
    text.push("i");
    return text;
}
```

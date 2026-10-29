# `std.rand_dist`

Sampling distributions over a `std.random.Random`.

The names carry a `sample_` prefix because a free function whose first
parameter is the receiver is reachable through method syntax as well, and a
free `exponential(generator, rate)` beside the method `Random.exponential`
made `generator.exponential(rate)` resolve to the free function -- which then
called itself. One name per operation: the methods on `Random`, and these
`sample_` spellings for code that reads better as a distribution name.

    generator Random = Random(7u32);
    sample f64 = rand_dist.sample_exponential(&generator, 2.0);

The generator owns the algorithm and the state; this module gives the
distributions their names as free functions, so a program can write
`rand_dist.normal(&generator, 0.0, 1.0)` and read like the statistic it means.
The methods on `Random` remain, and these call them: one implementation, two
spellings, no drift.

## Declarations

### `fn sample_uniform(generator &Random, low T, high T) T`

A uniform sample from `low` up to (not including) `high`.
Draws an integer in `[low, high]`, included at both ends.

- `generator`: the source of random numbers
- `low`: the smallest value the draw may give
- `high`: the largest, included

```
@enable_if(IsInteger(T))
export
template <T type>
fn sample_uniform(generator &Random, low T, high T) T
```

### `export fn sample_uniform_real(generator &Random, low f64, high f64) f64 {`

A uniform real in `[low, high)`.

```
export fn sample_uniform_real(generator &Random, low f64, high f64) f64 {
    return generator.uniform_real(low, high);
}
```

### `export fn sample_normal(generator &Random, mean f64, deviation f64) f64 {`

A normal (Gaussian) sample.

```
export fn sample_normal(generator &Random, mean f64, deviation f64) f64 {
    return generator.normal(mean, deviation);
}
```

### `export fn sample_exponential(generator &Random, rate f64) f64 {`

An exponential sample with the given rate.

```
export fn sample_exponential(generator &Random, rate f64) f64 {
    return generator.exponential(rate);
}
```

### `export fn sample_lognormal(generator &Random, mean f64, deviation f64) f64 {`

A log-normal sample.

```
export fn sample_lognormal(generator &Random, mean f64, deviation f64) f64 {
    return generator.lognormal(mean, deviation);
}
```

### `export fn sample_poisson(generator &Random, mean f64) uint {`

A Poisson sample with the given mean.

```
export fn sample_poisson(generator &Random, mean f64) uint {
    return generator.poisson(mean);
}
```

### `export fn sample_binomial(generator &Random, trials usize, chance f64) usize {`

A binomial sample: successes in `trials` with `chance` each.

```
export fn sample_binomial(generator &Random, trials usize, chance f64) usize {
    return generator.binomial(trials, chance);
}
```

### `export fn sample_geometric(generator &Random, chance f64) uint {`

A geometric sample: trials up to and including the first success.

```
export fn sample_geometric(generator &Random, chance f64) uint {
    return generator.geometric(chance);
}
```

### `export fn sample_bernoulli(generator &Random, chance f64) bool {`

A Bernoulli sample: true with `chance`.

```
export fn sample_bernoulli(generator &Random, chance f64) bool {
    return generator.bernoulli(chance);
}
```

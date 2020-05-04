// Copyright 2020 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

use sp_std::{ops::{self, Add, Sub, Mul, Div}, fmt::Debug, prelude::*, convert::{TryInto, TryFrom}};
use codec::{Encode, Decode};
use crate::{
	helpers_128bit::multiply_by_rational, PerThing,
	traits::{
		SaturatedConversion, CheckedSub, CheckedAdd, CheckedMul, CheckedDiv, CheckedNeg,
		Bounded, Saturating, UniqueSaturatedInto, Zero,
	},
};

#[cfg(feature = "std")]
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub trait FixedPointOperand: Copy + Clone + Bounded + Zero + Saturating
	+ PartialOrd + UniqueSaturatedInto<u128> + TryFrom<u128> + CheckedNeg {}

impl FixedPointOperand for i128 {}
impl FixedPointOperand for u128 {}
impl FixedPointOperand for i64 {}
impl FixedPointOperand for u64 {}
impl FixedPointOperand for i32 {}
impl FixedPointOperand for u32 {}
impl FixedPointOperand for i16 {}
impl FixedPointOperand for u16 {}
impl FixedPointOperand for i8 {}
impl FixedPointOperand for u8 {}

pub trait FixedPointNumber:
	Sized + Copy + Default + Debug
	+ Saturating + Bounded
	+ Eq + PartialEq + Ord + PartialOrd
	+ CheckedSub + CheckedAdd + CheckedMul + CheckedDiv
	+ Add + Sub + Div + Mul
{
	/// The underlying data type used for this fixed point number.
	type Inner: Copy + Debug;

	/// The accuracy of this fixed point number.
	const DIV: Self::Inner;

	/// Accuracy of this `Fixed` implementation.
	fn accuracy() -> Self::Inner {
		Self::DIV
	}

	/// Raw constructor. Equals to `int / DIV`.
	fn from_inner(int: Self::Inner) -> Self;

	/// Consumes `self` and returns the inner raw value.
	fn into_inner(self) -> Self::Inner;

	/// Creates self from an integer number `int`.
	/// 
	/// Saturates towards `max` or `min` if `int` exceeds accuracy.
	fn from_integer<N: UniqueSaturatedInto<Self::Inner>>(int: N) -> Self;

	/// Creates `self` from an integer number `int`.
	/// 
	/// Returns `None` if `int` exceeds accuracy.
	fn checked_from_integer(int: Self::Inner) -> Option<Self>;

	/// Creates `self` from a rational number. Equal to `n / d`.
	///
	/// Saturates if `d == 0` or `n / d` exceeds accuracy.
	fn from_rational<N: FixedPointOperand, D: FixedPointOperand>(n: N, d: D) -> Self;

	/// Creates `self` from a rational number. Equal to `n / d`.
	///
	/// Returns `None` if `d == 0` or `n / d` exceeds accuracy.
	fn checked_from_rational<N: FixedPointOperand, D: FixedPointOperand>(n: N, d: D) -> Option<Self>;

	/// Checked multiplication for integer type `N`.
	fn checked_mul_int<N: FixedPointOperand>(self, other: N) -> Option<N>;

	/// Saturating multiplication for integer type `N`.
	fn saturating_mul_int<N: FixedPointOperand>(self, other: N) -> N;

	/// Checked division for integer type `N`.
	fn checked_div_int<N: FixedPointOperand>(self, other: N) -> Option<N>;

	/// Saturating division for integer type `N`.
	fn saturating_div_int<N: FixedPointOperand>(self, other: N) -> N;

	/// Performs a saturated multiplication and accumulate by unsigned number.
	///
	/// Returns a saturated `int + (self * int)`.
	fn saturating_mul_int_acc<N: FixedPointOperand>(self, int: N) -> N;

	/// Saturating absolute value. Returning MAX if `parts == Inner::MIN` instead of overflowing.
	fn saturating_abs(self) -> Self;

	/// Takes the reciprocal (inverse), `1 / self`.
	fn reciprocal(self) -> Option<Self> {
		Self::one().checked_div(&self)
	}

	/// Returns zero.
	fn zero() -> Self;

	/// Checks if the number is zero.
	fn is_zero(&self) -> bool;

	/// Returns one.
	fn one() -> Self;

	/// Checks if the number is positive.
	fn is_positive(self) -> bool;

	/// Checks if the number is negative.
	fn is_negative(self) -> bool;

	/// Returns the integer part.
	fn trunc(self) -> Self;

	/// Returns the fractional part.
	fn frac(self) -> Self;

	/// Returns the smallest integer greater than or equal to a number.
	fn ceil(self) -> Self;

	/// Returns the largest integer less than or equal to a number.
	fn floor(self) -> Self;

	/// Returns the nearest integer to a number. Round half-way cases away from 0.0.
	fn round(self) -> Self;

	/// Computes the absolute value of self.
	fn abs(self) -> Self;	
}

struct I129 {
	value: u128,
	negative: bool,
}

impl<N: FixedPointOperand> From<N> for I129 {
	fn from(n: N) -> I129 {
		if n < N::zero() {
			let value: u128 = n.checked_neg()
				.map(|n| n.unique_saturated_into())
				.unwrap_or(N::max_value().unique_saturated_into().saturating_add(1));
			I129 { value, negative: true }
		} else {
			I129 { value: n.unique_saturated_into(), negative: false }
		}
	}
}

/// Transforms an `I129` to `N` if it is possible.
fn from_i129<N: FixedPointOperand>(n: I129) -> Option<N> {
	let max_plus_one: u128 = N::max_value().unique_saturated_into().saturating_add(1);
	if n.negative && N::min_value() < N::zero() && n.value == max_plus_one {
		Some(N::min_value())
	} else {
		let unsigned_inner: N = n.value.try_into().ok()?;
		let inner = if n.negative { unsigned_inner.checked_neg()? } else { unsigned_inner };
		Some(inner)
	}
}

/// Returns `R::max` or `R::min` depending of the operand signs.
fn to_bound<N: FixedPointOperand, D: FixedPointOperand, R: Bounded>(n: N, m: D) -> R {
	if (n < N::zero()) != (m < D::zero()) {
		R::min_value()
	} else {
		R::max_value()
	}
}

macro_rules! implement_fixed {
	(
		$name:ident,
		$test_mod:ident,
		$inner_type:ty,
		$div:tt,
	) => {
		#[derive(Encode, Decode, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
		pub struct $name($inner_type);

		impl From<$inner_type> for $name {
			fn from(int: $inner_type) -> Self {
				$name::from_integer(int)
			}
		}

		impl<N: FixedPointOperand, D: FixedPointOperand> From<(N, D)> for $name {
			fn from(r: (N, D)) -> Self {
				$name::from_rational(r.0, r.1)
			}
		}

		impl FixedPointNumber for $name {
			type Inner = $inner_type;

			const DIV: Self::Inner = $div;

			fn from_inner(inner: Self::Inner) -> Self {
				Self(inner)
			}

			fn into_inner(self) -> Self::Inner {
				self.0
			}

			fn from_integer<N: UniqueSaturatedInto<Self::Inner>>(int: N) -> Self {
				Self(int.unique_saturated_into().saturating_mul(Self::DIV))
			}

			fn checked_from_integer(int: Self::Inner) -> Option<Self> {
				int.checked_mul(Self::DIV).map(Self)
			}

			fn trunc(self) -> Self {
				Self(self.0 / Self::DIV * Self::DIV)
			}

			fn frac(self) -> Self {
				self.saturating_sub(self.trunc()).abs()
			}

			fn ceil(self) -> Self {
				if self.is_negative() {
					self.trunc()
				} else {
					self.saturating_add(Self::one()).trunc()
				}
			}

			fn floor(self) -> Self {
				if self.is_negative() {
					self.saturating_sub(Self::one()).trunc()
				} else {
					self.trunc()
				}
			}

			fn round(self) -> Self {
				let n = self.frac() * 10.into();
				if n < 5.into() {
					self.trunc()
				} else {
					let extra: Self = self.0.signum().into();
					(self + extra).trunc()
				}
			}

			fn abs(self) -> Self {
				Self(self.0.checked_abs().unwrap_or(Self::Inner::max_value()))
			}

			fn zero() -> Self {
				Self(0)
			}

			fn is_zero(&self) -> bool {
				self.0 == 0
			}

			fn one() -> Self {
				Self(Self::DIV)
			}

			fn is_positive(self) -> bool {
				self.0.is_positive()
			}

			fn is_negative(self) -> bool {
				self.0.is_negative()
			}

			fn checked_from_rational<N: FixedPointOperand, D: FixedPointOperand>(n: N, d: D) -> Option<Self> {
				let n: I129 = n.into();
				let d: I129 = d.into();
				let negative = n.negative != d.negative;

				multiply_by_rational(n.value, Self::DIV as u128, d.value).ok()
					.and_then(|value| from_i129(I129 { value, negative }))
					.map(Self)
			}

			fn from_rational<N: FixedPointOperand, D: FixedPointOperand>(n: N, d: D) -> Self {
				Self::checked_from_rational(n, d).unwrap_or(to_bound(n, d))
			}

			fn checked_mul_int<N: FixedPointOperand>(self, int: N) -> Option<N>
			{
				let lhs: I129 = self.0.into();
				let rhs: I129 = int.into();
				let negative = lhs.negative != rhs.negative;

				multiply_by_rational(lhs.value, rhs.value, Self::DIV as u128).ok()
					.and_then(|value| from_i129(I129 { value, negative }))
			}

			fn checked_div_int<N: FixedPointOperand>(self, int: N) -> Option<N>
			{
				let lhs: I129 = self.0.into();
				let rhs: I129 = int.into();
				let negative = lhs.negative != rhs.negative;

				lhs.value.checked_div(rhs.value)
					.and_then(|n| n.checked_div(Self::DIV as u128))
					.and_then(|value| from_i129(I129 { value, negative }))
			}

			fn saturating_div_int<N: FixedPointOperand>(self, int: N) -> N
			{
				self.checked_div_int(int).unwrap_or(to_bound(self.0, int))
			}

			fn saturating_mul_int<N: FixedPointOperand>(self, int: N) -> N
			{
				self.checked_mul_int(int).unwrap_or(to_bound(self.0, int))
			}

			fn saturating_abs(self) -> Self {
				Self(self.0.checked_abs().unwrap_or(Self::Inner::max_value()))
			}

			fn saturating_mul_int_acc<N: FixedPointOperand>(self, int: N) -> N
			{
				self.saturating_mul_int(int).saturating_add(int)
			}
		}

		impl Saturating for $name {
			fn saturating_add(self, rhs: Self) -> Self {
				Self(self.0.saturating_add(rhs.0))
			}

			fn saturating_sub(self, rhs: Self) -> Self {
				Self(self.0.saturating_sub(rhs.0))
			}

			fn saturating_mul(self, rhs: Self) -> Self {
				self.checked_mul(&rhs).unwrap_or(to_bound(self.0, rhs.0))
			}

			fn saturating_pow(self, exp: usize) -> Self {
				if exp == 0 {
					return Self::from_integer(1);
				}

				let exp = exp as u32;
				let msb_pos = 32 - exp.leading_zeros();

				let mut result = Self::from_integer(1);
				let mut pow_val = self;
				for i in 0..msb_pos {
					if ((1 << i) & exp) > 0 {
						result = result.saturating_mul(pow_val);
					}
					pow_val = pow_val.saturating_mul(pow_val);
				}
				result
			}
		}

		impl ops::Neg for $name {
			type Output = Self;

			fn neg(self) -> Self::Output {
				Self(-self.0)
			}
		}

		impl ops::Add for $name {
			type Output = Self;

			fn add(self, rhs: Self) -> Self::Output {
				Self(self.0 + rhs.0)
			}
		}

		impl ops::Sub for $name {
			type Output = Self;

			fn sub(self, rhs: Self) -> Self::Output {
				Self(self.0 - rhs.0)
			}
		}

		impl ops::Mul for $name {
			type Output = Self;

			fn mul(self, rhs: Self) -> Self::Output {
				self.checked_mul(&rhs)
					.unwrap_or_else(|| panic!("attempt to multiply with overflow"))
			}
		}

		impl ops::Div for $name {
			type Output = Self;

			fn div(self, rhs: Self) -> Self::Output {
				if rhs.0 == 0 {
					panic!("attempt to divide by zero")
				}
				self.checked_div(&rhs)
					.unwrap_or_else(|| panic!("attempt to divide with overflow"))
			}
		}

		impl CheckedSub for $name {
			fn checked_sub(&self, rhs: &Self) -> Option<Self> {
				self.0.checked_sub(rhs.0).map(Self)
			}
		}

		impl CheckedAdd for $name {
			fn checked_add(&self, rhs: &Self) -> Option<Self> {
				self.0.checked_add(rhs.0).map(Self)
			}
		}

		impl CheckedDiv for $name {
			fn checked_div(&self, other: &Self) -> Option<Self> {
				if other.0 == 0 {
					return None
				}

				let lhs: I129 = self.0.into();
				let rhs: I129 = other.0.into();
				let negative = lhs.negative != rhs.negative;

				multiply_by_rational(lhs.value, Self::DIV as u128, rhs.value).ok()
					.and_then(|value| from_i129(I129 { value, negative }))
					.map(Self)
			}
		}

		impl CheckedMul for $name {
			fn checked_mul(&self, other: &Self) -> Option<Self> {
				let lhs: I129 = self.0.into();
				let rhs: I129 = other.0.into();
				let negative = lhs.negative != rhs.negative;

				multiply_by_rational(lhs.value, rhs.value, Self::DIV as u128).ok()
					.and_then(|value| from_i129(I129 { value, negative }))
					.map(Self)
			}
		}

		impl Bounded for $name {
			fn min_value() -> Self {
				Self(<Self as FixedPointNumber>::Inner::min_value())
			}

			fn max_value() -> Self {
				Self(<Self as FixedPointNumber>::Inner::max_value())
			}
		}

		impl sp_std::fmt::Debug for $name {
			#[cfg(feature = "std")]
			fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
				let integral = {
					let int = self.0 / Self::accuracy();
					let signum_for_zero = if int == 0 && self.is_negative() { "-" } else { "" };
					format!("{}{}", signum_for_zero, int)
				};
				let precision = (Self::accuracy() as f64).log10() as usize;
				let fractional = format!("{:0>weight$}", (self.0 % Self::accuracy()).abs(), weight=precision);
				write!(f, "{}({}.{})", stringify!($name), integral, fractional)
			}

			#[cfg(not(feature = "std"))]
			fn fmt(&self, _: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
				Ok(())
			}
		}

		impl<P: PerThing> From<P> for $name {
			fn from(p: P) -> Self {
				let accuracy = P::ACCURACY.saturated_into() as <Self as FixedPointNumber>::Inner;
				let value = p.deconstruct().saturated_into() as <Self as FixedPointNumber>::Inner;
				$name::from_rational(value, accuracy)
			}
		}

		#[cfg(feature = "std")]
		impl sp_std::fmt::Display for $name {
			fn fmt(&self, f: &mut sp_std::fmt::Formatter) -> sp_std::fmt::Result {
				write!(f, "{}", self.0)
			}
		}

		#[cfg(feature = "std")]
		impl sp_std::str::FromStr for $name {
			type Err = &'static str;

			fn from_str(s: &str) -> Result<Self, Self::Err> {
				let inner: <Self as FixedPointNumber>::Inner = s.parse()
					.map_err(|_| "invalid string input for fixed point number")?;
				Ok(Self::from_inner(inner))
			}
		} 

		// Manual impl `Serialize` as serde_json does not support i128.
		// TODO: remove impl if issue https://github.com/serde-rs/json/issues/548 fixed.
		#[cfg(feature = "std")]
		impl Serialize for $name {
			fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
			where
				S: Serializer,
			{
				serializer.serialize_str(&self.to_string())
			}
		}

		// Manual impl `Serialize` as serde_json does not support i128.
		// TODO: remove impl if issue https://github.com/serde-rs/json/issues/548 fixed.
		#[cfg(feature = "std")]
		impl<'de> Deserialize<'de> for $name {
			fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
			where
				D: Deserializer<'de>,
			{
				use sp_std::str::FromStr;
				let s = String::deserialize(deserializer)?;
				$name::from_str(&s).map_err(|err_str| de::Error::custom(err_str))
			}
		}

		#[cfg(test)]
		mod $test_mod {
			use super::*;
			use crate::{Perbill, Percent, Permill, Perquintill};

			fn max() -> $name {
				$name::max_value()
			}

			fn min() -> $name {
				$name::min_value()
			}

			fn precision() -> usize {
				($name::accuracy() as f64).log10() as usize
			}

			#[test]
			fn from_i129_works() {
				let a = I129 {
					value: 1,
					negative: true,
				};

				assert_eq!(from_i129::<u128>(a), None);

				let a = I129 {
					value: u128::max_value(),
					negative: false,
				};

				assert_eq!(from_i129::<u128>(a), Some(u128::max_value()));

				let a = I129 {
					value: i128::max_value() as u128 + 1,
					negative: true,
				};

				assert_eq!(from_i129::<i128>(a), Some(i128::min_value()));

				let a = I129 {
					value: i128::max_value() as u128 + 1,
					negative: false,
				};

				assert_eq!(from_i129::<i128>(a), None);

				let a = I129 {
					value: i128::max_value() as u128,
					negative: false,
				};

				assert_eq!(from_i129::<i128>(a), Some(i128::max_value()));
			}

			#[test]
			fn to_bound_works() {
				let a = 1i32;
				let b = 1i32;
				assert_eq!(to_bound::<_, _, i32>(a, b), i32::max_value());

				let a = 1i32;
				let b = -1i32;
				assert_eq!(to_bound::<_, _, i32>(a, b), i32::min_value());

				let a = -1i32;
				let b = 1i32;
				assert_eq!(to_bound::<_, _, i32>(a, b), i32::min_value());


				let a = 1i32;
				let b = 1i32;
				assert_eq!(to_bound::<_, _, u32>(a, b), u32::max_value());

				let a = 1i32;
				let b = -1i32;
				assert_eq!(to_bound::<_, _, u32>(a, b), 0);
			}

			#[test]
			#[should_panic(expected = "attempt to negate with overflow")]
			fn op_neg_panics() {
				let a = $name::min_value();
				let b = -a;
				assert_eq!(b, a);
			}

			#[test]
			fn op_neg_works() {
				let a = $name::from_integer(5);
				let b = -a;
				assert_eq!($name::from_integer(-5), b);
			}

			#[test]
			#[should_panic(expected = "attempt to add with overflow")]
			fn op_add_panics() {
				let a = $name::max_value();
				let b = 1.into();
				let _c = a + b;
			}

			#[test]
			fn op_add_works() {
				let a = $name::from_integer(40);
				let b = $name::from_integer(2);
				assert_eq!($name::from_integer(42), a + b);

				let a = $name::from_integer(44);
				let b = $name::from_integer(-2);
				assert_eq!($name::from_integer(42), a + b);
			}

			#[test]
			#[should_panic(expected = "attempt to subtract with overflow")]
			fn op_sub_panics() {
				let a = $name::min_value();
				let b = 1.into();
				let _c = a - b;
			}

			#[test]
			fn op_sub_works() {
				let a = $name::from_integer(44);
				let b = $name::from_integer(2);
				assert_eq!($name::from_integer(42), a - b);

				let a = $name::from_integer(40);
				let b = $name::from_integer(-2);
				assert_eq!($name::from_integer(42), a - b);
			}

			#[test]
			#[should_panic(expected = "attempt to multiply with overflow")]
			fn op_mul_panics() {
				let a = $name::max_value();
				let b = 2.into();
				let _c = a * b;
			}

			#[test]
			fn op_mul_works() {
				let a = $name::from_integer(42);
				let b = $name::from_integer(2);
				assert_eq!($name::from_integer(84), a * b);

				let a = $name::from_integer(42);
				let b = $name::from_integer(-2);
				assert_eq!($name::from_integer(-84), a * b);
			}

			#[test]
			#[should_panic(expected = "attempt to divide by zero")]
			fn op_div_panics_on_zero_divisor() {
				let a = $name::from_integer(1);
				let b = 0.into();
				let _c = a / b;
			}

			#[test]
			#[should_panic(expected = "attempt to divide with overflow")]
			fn op_div_panics_on_overflow() {
				let a = $name::min_value();
				let b = (-1).into();
				let _c = a / b;
			}

			#[test]
			fn op_div_works() {
				let a = $name::from_integer(42);
				let b = $name::from_integer(2);
				assert_eq!($name::from_integer(21), a / b);

				let a = $name::from_integer(42);
				let b = $name::from_integer(-2);
				assert_eq!($name::from_integer(-21), a / b);
			}

			#[test]
			fn from_integer_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();
				let accuracy = $name::accuracy();

				// Cases where integer fits.
				let a = $name::from_integer(42);
				assert_eq!(a.into_inner(), 42 * accuracy);

				let a = $name::from_integer(-42);
				assert_eq!(a.into_inner(), -42 * accuracy);

				// Max/min integers that fit.
				let a = $name::from_integer(inner_max / accuracy);
				assert_eq!(a.into_inner(), (inner_max / accuracy) * accuracy);

				let a = $name::from_integer(inner_min / accuracy);
				assert_eq!(a.into_inner(), (inner_min / accuracy) * accuracy);

				// Cases where integer doesn't fit, so it saturates.
				let a = $name::from_integer(inner_max / accuracy + 1);
				assert_eq!(a.into_inner(), inner_max);

				let a = $name::from_integer(inner_min / accuracy - 1);
				assert_eq!(a.into_inner(), inner_min);
			}

			#[test]
			fn checked_from_integer_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();
				let accuracy = $name::accuracy();

				// Cases where integer fits.
				let a = $name::checked_from_integer(42)
					.expect("42 * accuracy <= inner_max; qed");
				assert_eq!(a.into_inner(), 42 * accuracy);

				let a = $name::checked_from_integer(-42)
					.expect("-42 * accuracy >= inner_min; qed");
				assert_eq!(a.into_inner(), -42 * accuracy);

				// Max/min integers that fit.
				let a = $name::checked_from_integer(inner_max / accuracy)
					.expect("(inner_max / accuracy) * accuracy <= inner_max; qed");
				assert_eq!(a.into_inner(), (inner_max / accuracy) * accuracy);

				let a = $name::checked_from_integer(inner_min / accuracy)
					.expect("(inner_min / accuracy) * accuracy <= inner_min; qed");
				assert_eq!(a.into_inner(), (inner_min / accuracy) * accuracy);

				// Cases where integer doesn't fit, so it returns `None`.
				let a = $name::checked_from_integer(inner_max / accuracy + 1);
				assert_eq!(a, None);

				let a = $name::checked_from_integer(inner_min / accuracy - 1);
				assert_eq!(a, None);
			}

			#[test]
			fn from_inner_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();

				assert_eq!(max(), $name::from_inner(inner_max));
				assert_eq!(min(), $name::from_inner(inner_min));
			}

			#[test]
			fn from_rational_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();
				let accuracy = $name::accuracy();

				let a = $name::from_rational(5, 2);
				let b = $name::from_rational(-5, 2);
				let c = $name::from_rational(5, -2);
				let d = $name::from_rational(-5, -2);

				let e = $name::from_rational(inner_max, accuracy);
				let f = $name::from_rational(inner_min, accuracy);
				let g = $name::from_rational(inner_max, -accuracy);
				let h = $name::from_rational(inner_min, -accuracy);
				let i = $name::from_rational(inner_min + 1, -accuracy);

				let k = $name::from_rational(inner_max, 1);
				let l = $name::from_rational(inner_min, 1);
				let m = $name::from_rational(inner_min, -1);
				let n = $name::from_rational(inner_max, -1);

				let o = $name::from_rational(inner_max, inner_max);
				let p = $name::from_rational(inner_min, inner_min);
				let r = $name::from_rational(inner_max, -inner_max);
				let s = $name::from_rational(-inner_max, inner_max);

				let t = $name::from_rational(inner_max, 3 * accuracy);
				let u = $name::from_rational(inner_max, -3 * accuracy);

				let v = $name::from_rational(inner_min, 2 * accuracy);

				let w = $name::from_rational(inner_min, accuracy / -3);
				let x = $name::from_rational(inner_min, accuracy / 3);

				let y = $name::from_rational(1, accuracy);
				let z = $name::from_rational(1, -accuracy);

				assert_eq!(a.into_inner(), 25 * accuracy / 10);
				assert_eq!(b.saturating_mul_int(10),-25);
				assert_eq!(c.saturating_mul_int(10), -25);
				assert_eq!(d.saturating_mul_int(10), 25);

				assert_eq!(e.into_inner(), inner_max);
				assert_eq!(f.into_inner(), inner_min);
				assert_eq!(g.into_inner(), -inner_max);
				assert_eq!(h.into_inner(), inner_max);
				assert_eq!(i.into_inner(), inner_max);

				assert_eq!(k.into_inner(), inner_max);
				assert_eq!(l.into_inner(), inner_min);
				assert_eq!(m.into_inner(), inner_max);
				assert_eq!(n.into_inner(), inner_min);

				assert_eq!(o.into_inner(), accuracy);
				assert_eq!(p.into_inner(), accuracy);
				assert_eq!(r.into_inner(), -accuracy);
				assert_eq!(s.into_inner(), -accuracy);

				assert_eq!(t.into_inner(), inner_max / 3);
				assert_eq!(u.into_inner(), -inner_max / 3);

				assert_eq!(v.into_inner(), inner_min / 2);
				assert_eq!(w.into_inner(), inner_max);
				assert_eq!(x.into_inner(), inner_min);

				assert_eq!(y.into_inner(), 1);
				assert_eq!(z.into_inner(), -1);

				let a = $name::from_rational(1, accuracy + 1);
				let b = $name::from_rational(1, -accuracy - 1);

				assert_eq!(a.into_inner(), 0);
				assert_eq!(b.into_inner(), 0);
			}

			#[test]
			fn checked_mul_int_works() {
				let a = $name::from_rational(1, 2);
				let b = $name::from_rational(1, -2);
				let c = $name::from_integer(255);

				assert_eq!(a.checked_mul_int(42i128), Some(21));
				assert_eq!(b.checked_mul_int(42i128), Some(-21));
				assert_eq!(c.checked_mul_int(2i128), Some(510));

				assert_eq!(b.checked_mul_int(i128::max_value()), Some(i128::max_value() / -2));
				assert_eq!(b.checked_mul_int(i128::min_value()), Some(i128::min_value() / -2));

				assert_eq!(c.checked_mul_int(i128::max_value()), None);
				assert_eq!(c.checked_mul_int(i128::min_value()), None);

				assert_eq!(a.checked_mul_int(i128::max_value()), Some(i128::max_value() / 2));
				assert_eq!(a.checked_mul_int(u64::max_value()), Some(u64::max_value() / 2));
				assert_eq!(a.checked_mul_int(i128::min_value()), Some(i128::min_value() / 2));
			}

			#[test]
			fn checked_div_int_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();
				let accuracy = $name::accuracy();

				let a = $name::from_inner(inner_max);
				let b = $name::from_inner(inner_min);
				let c = $name::zero();
				let d = $name::one();
				let e = $name::from_integer(6);
				let f = $name::from_integer(5);

				assert_eq!(e.checked_div_int(2.into()), Some(3));
				assert_eq!(f.checked_div_int(2.into()), Some(2));

				assert_eq!(a.checked_div_int(i128::max_value()), Some(0));
				assert_eq!(a.checked_div_int(2), Some(inner_max / (2 * accuracy)));
				assert_eq!(a.checked_div_int(inner_max / accuracy), Some(1));
				assert_eq!(a.checked_div_int(1i8), None);

				assert_eq!(a.checked_div_int(-2), Some(-inner_max / (2 * accuracy)));
				assert_eq!(a.checked_div_int(inner_max / -accuracy), Some(-1));

				assert_eq!(b.checked_div_int(i128::min_value()), Some(0));
				assert_eq!(b.checked_div_int(2), Some(inner_min / (2 * accuracy)));
				assert_eq!(b.checked_div_int(inner_min / accuracy), Some(1));
				assert_eq!(b.checked_div_int(1i8), None);

				assert_eq!(b.checked_div_int(-2), Some(-(inner_min / (2 * accuracy))));
				assert_eq!(b.checked_div_int(-(inner_min / accuracy)), Some(-1));

				assert_eq!(c.checked_div_int(1), Some(0));
				assert_eq!(c.checked_div_int(i128::max_value()), Some(0));
				assert_eq!(c.checked_div_int(i128::min_value()), Some(0));
				assert_eq!(c.checked_div_int(1i8), Some(0));

				assert_eq!(d.checked_div_int(1), Some(1));
				assert_eq!(d.checked_div_int(i32::max_value()), Some(0));
				assert_eq!(d.checked_div_int(i32::min_value()), Some(0));
				assert_eq!(d.checked_div_int(1i8), Some(1));

				assert_eq!(a.checked_div_int(0), None);
				assert_eq!(b.checked_div_int(0), None);
				assert_eq!(c.checked_div_int(0), None);
				assert_eq!(d.checked_div_int(0), None);
			}

			#[test]
			fn saturating_mul_int_works() {
				let a = $name::from_rational(1, 2);
				let b = $name::from_rational(1, -2);
				let c = $name::from_integer(255);
				let d = $name::from_integer(-1);

				assert_eq!(a.saturating_mul_int(42i32), 21);
				assert_eq!(b.saturating_mul_int(42i32), -21);

				assert_eq!(c.saturating_mul_int(2i8), i8::max_value());
				assert_eq!(c.saturating_mul_int(-2i8), i8::min_value());

				assert_eq!(b.saturating_mul_int(i128::max_value()), i128::max_value() / -2);
				assert_eq!(b.saturating_mul_int(i128::min_value()), i128::min_value() / -2);

				assert_eq!(c.saturating_mul_int(i128::max_value()), i128::max_value());
				assert_eq!(c.saturating_mul_int(i128::min_value()), i128::min_value());

				assert_eq!(a.saturating_mul_int(i128::max_value()), i128::max_value() / 2);
				assert_eq!(a.saturating_mul_int(u64::max_value()), u64::max_value() / 2);
				assert_eq!(a.saturating_mul_int(i128::min_value()), i128::min_value() / 2);

				assert_eq!(d.saturating_mul_int(i8::max_value()), -i8::max_value());
				assert_eq!(d.saturating_mul_int(i8::min_value()), i8::max_value());
			}

			#[test]
			fn saturating_abs_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();

				assert_eq!($name::from_inner(inner_min).saturating_abs(), $name::max_value());
				assert_eq!($name::from_inner(inner_max).saturating_abs(), $name::max_value());
				assert_eq!($name::zero().saturating_abs(), 0.into());
				assert_eq!($name::from_rational(-1, 2).saturating_abs(), (1, 2).into());
			}

			#[test]
			fn saturating_mul_int_acc_works() {
				let accuracy = $name::accuracy();

				assert_eq!($name::zero().saturating_mul_int_acc(accuracy as u64), accuracy as u64);
				assert_eq!($name::one().saturating_mul_int_acc(accuracy as u64), 2 * accuracy as u64);
				assert_eq!($name::one().saturating_mul_int_acc(i128::min_value()), i128::min_value());
			}

			#[test]
			fn saturating_pow_should_work() {
				assert_eq!($name::from_integer(2).saturating_pow(0), $name::from_integer(1));
				assert_eq!($name::from_integer(2).saturating_pow(1), $name::from_integer(2));
				assert_eq!($name::from_integer(2).saturating_pow(2), $name::from_integer(4));
				assert_eq!($name::from_integer(2).saturating_pow(3), $name::from_integer(8));
				assert_eq!($name::from_integer(2).saturating_pow(50), $name::from_integer(1125899906842624i64));

				// Saturating.
				assert_eq!($name::from_integer(2).saturating_pow(68), $name::max_value());

				assert_eq!($name::from_integer(1).saturating_pow(1000), $name::from_integer(1));
				assert_eq!($name::from_integer(-1).saturating_pow(1000), $name::from_integer(1));
				assert_eq!($name::from_integer(-1).saturating_pow(1001), $name::from_integer(-1));
				assert_eq!($name::from_integer(1).saturating_pow(usize::max_value()), $name::from_integer(1));
				assert_eq!($name::from_integer(-1).saturating_pow(usize::max_value()), $name::from_integer(-1));
				assert_eq!($name::from_integer(-1).saturating_pow(usize::max_value() - 1), $name::from_integer(1));

				assert_eq!($name::from_integer(114209).saturating_pow(5), $name::max_value());

				assert_eq!($name::from_integer(1).saturating_pow(usize::max_value()), $name::from_integer(1));
				assert_eq!($name::from_integer(0).saturating_pow(usize::max_value()), $name::from_integer(0));
				assert_eq!($name::from_integer(2).saturating_pow(usize::max_value()), $name::max_value());
			}

			#[test]
			fn checked_div_works() {
				let inner_max = <$name as FixedPointNumber>::Inner::max_value();
				let inner_min = <$name as FixedPointNumber>::Inner::min_value();

				let a = $name::from_inner(inner_max);
				let b = $name::from_inner(inner_min);
				let c = $name::zero();
				let d = $name::one();
				let e = $name::from_integer(6);
				let f = $name::from_integer(5);

				assert_eq!(e.checked_div(&2.into()), Some(3.into()));
				assert_eq!(f.checked_div(&2.into()), Some((5, 2).into()));

				assert_eq!(a.checked_div(&inner_max.into()), Some(1.into()));
				assert_eq!(a.checked_div(&2.into()), Some($name::from_inner(inner_max / 2)));
				assert_eq!(a.checked_div(&$name::max_value()), Some(1.into()));
				assert_eq!(a.checked_div(&d), Some(a));

				assert_eq!(a.checked_div(&(-2).into()), Some($name::from_inner(-inner_max / 2)));
				assert_eq!(a.checked_div(&-$name::max_value()), Some((-1).into()));

				assert_eq!(b.checked_div(&b), Some($name::one()));
				assert_eq!(b.checked_div(&2.into()), Some($name::from_inner(inner_min / 2)));

				assert_eq!(b.checked_div(&(-2).into()), Some($name::from_inner(inner_min / -2)));
				assert_eq!(b.checked_div(&a), Some((-1).into()));

				assert_eq!(c.checked_div(&1.into()), Some(0.into()));
				assert_eq!(c.checked_div(&$name::max_value()), Some(0.into()));
				assert_eq!(c.checked_div(&$name::min_value()), Some(0.into()));

				assert_eq!(d.checked_div(&1.into()), Some(1.into()));
				assert_eq!(d.checked_div(&$name::max_value()), Some(0.into()));
				assert_eq!(d.checked_div(&$name::min_value()), Some(0.into()));

				assert_eq!(a.checked_div(&$name::one()), Some(a));
				assert_eq!(b.checked_div(&$name::one()), Some(b));
				assert_eq!(c.checked_div(&$name::one()), Some(c));
				assert_eq!(d.checked_div(&$name::one()), Some(d));

				assert_eq!(a.checked_div(&$name::zero()), None);
				assert_eq!(b.checked_div(&$name::zero()), None);
				assert_eq!(c.checked_div(&$name::zero()), None);
				assert_eq!(d.checked_div(&$name::zero()), None);
			}

			#[test]
			fn checked_mul_works() {
				let a = $name::from_rational(1, 2);
				let b = $name::from_rational(1, -2);
				let c = $name::from_integer(255);

				assert_eq!(a.checked_mul(&42.into()), Some(21.into()));
				assert_eq!(b.checked_mul(&42.into()), Some((-21).into()));
				assert_eq!(c.checked_mul(&2.into()), Some(510.into()));

				assert_eq!(b.checked_mul(&$name::max_value()), $name::max_value().checked_div(&(-2).into()));
				assert_eq!(b.checked_mul(&$name::min_value()), $name::min_value().checked_div(&(-2).into()));

				assert_eq!(c.checked_mul(&$name::max_value()), None);
				assert_eq!(c.checked_mul(&$name::min_value()), None);

				assert_eq!(a.checked_mul(&$name::max_value()), $name::max_value().checked_div(&2.into()));
				assert_eq!(a.checked_mul(&$name::min_value()), $name::min_value().checked_div(&2.into()));
			} 

			#[test]
			fn mul_works() {
				let a = $name::from_integer(1);
				let b = $name::from_integer(2);
				let c = a * b;
				assert_eq!(c, b);
			}

			#[test]
			fn trunc_works() {
				let n = $name::from_rational(5, 2).trunc();
				assert_eq!(n, $name::from_integer(2));

				let n = $name::from_rational(-5, 2).trunc();
				assert_eq!(n, $name::from_integer(-2));
			}

			#[test]
			fn frac_works() {
				let n = $name::from_rational(5, 2);
				let i = n.trunc();
				let f = n.frac();

				assert_eq!(n, i + f);

				let n = $name::from_rational(-5, 2);
				let i = n.trunc();
				let f = n.frac();

				assert_eq!(n, i - f);

				let n = $name::from_rational(5, 2)
					.frac()
					.saturating_mul(10.into());
				assert_eq!(n, 5.into());
			}

			#[test]
			fn ceil_works() {
				let n = $name::from_rational(5, 2);
				assert_eq!(n.ceil(), 3.into());

				let n = $name::from_rational(-5, 2);
				assert_eq!(n.ceil(), (-2).into());
			}

			#[test]
			fn floor_works() {
				let n = $name::from_rational(5, 2);
				assert_eq!(n.floor(), 2.into());

				let n = $name::from_rational(-5, 2);
				assert_eq!(n.floor(), (-3).into());
			}

			#[test]
			fn round_works() {
				// let n = $name::from_rational(5, 2);
				// assert_eq!(n.round(), 3.into());

				let n = $name::from_rational(-5, 2);
				assert_eq!(n.round(), (-3).into());
			}

			#[test]
			fn perthing_into_works() {
				let ten_percent_percent: $name = Percent::from_percent(10).into();
				assert_eq!(ten_percent_percent.into_inner(), $name::accuracy() / 10);

				let ten_percent_permill: $name = Permill::from_percent(10).into();
				assert_eq!(ten_percent_permill.into_inner(), $name::accuracy() / 10);

				let ten_percent_perbill: $name = Perbill::from_percent(10).into();
				assert_eq!(ten_percent_perbill.into_inner(), $name::accuracy() / 10);

				let ten_percent_perquintill: $name = Perquintill::from_percent(10).into();
				assert_eq!(ten_percent_perquintill.into_inner(), $name::accuracy() / 10);
			}

			#[test]
			fn fmt_should_work() {
				let zero = $name::zero();
				assert_eq!(format!("{:?}", zero), format!("{}(0.{:0>weight$})", stringify!($name), 0, weight=precision()));

				let one = $name::one();
				assert_eq!(format!("{:?}", one), format!("{}(1.{:0>weight$})", stringify!($name), 0, weight=precision()));

				let neg = -$name::one();
				assert_eq!(format!("{:?}", neg), format!("{}(-1.{:0>weight$})", stringify!($name), 0, weight=precision()));

				let frac = $name::from_rational(1, 2);
				assert_eq!(format!("{:?}", frac), format!("{}(0.{:0<weight$})", stringify!($name), 5, weight=precision()));

				let frac = $name::from_rational(5, 2);
				assert_eq!(format!("{:?}", frac), format!("{}(2.{:0<weight$})", stringify!($name), 5, weight=precision()));

				let frac = $name::from_rational(314, 100);
				assert_eq!(format!("{:?}", frac), format!("{}(3.{:0<weight$})", stringify!($name), 14, weight=precision()));

				let frac = $name::from_rational(-314, 100);
				assert_eq!(format!("{:?}", frac), format!("{}(-3.{:0<weight$})", stringify!($name), 14, weight=precision()));
			}
		}
	}
}

implement_fixed!(
	Fixed64,
	test_fixed64,
	i64,
	1_000_000_000,
);

implement_fixed!(
	Fixed128,
	test_fixed128,
	i128,
	1_000_000_000_000_000_000,
);

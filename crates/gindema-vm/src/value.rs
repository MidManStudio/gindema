// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "value.rs"
// ============================================================================

use crate::error::VmError;

/// A value gindema-vm's stack, constant pool, and local slots hold.
///
/// Deliberately small for this first pass: no strings, no heap objects, no
/// arrays — just enough to express real arithmetic and control flow. See
/// `docs/gindema-vm.md` for what's intentionally not here yet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A 64-bit signed integer.
    Int(i64),
    /// A 64-bit float.
    Float(f64),
    /// A boolean, produced by comparisons and consumed by `JumpIfFalse`.
    Bool(bool),
}

impl Value {
    /// Returns the boolean if this is [`Value::Bool`], `None` otherwise.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Adds two values of the same numeric kind.
    ///
    /// # Errors
    /// [`VmError::TypeMismatch`] if the two values aren't both `Int` or both
    /// `Float`. [`VmError::ArithmeticOverflow`] on `Int` overflow.
    pub fn try_add(self, other: Self) -> Result<Self, VmError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a
                .checked_add(b)
                .map(Self::Int)
                .ok_or(VmError::ArithmeticOverflow),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Float(a + b)),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Subtracts two values of the same numeric kind. Same error conditions
    /// as [`Value::try_add`].
    pub fn try_sub(self, other: Self) -> Result<Self, VmError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a
                .checked_sub(b)
                .map(Self::Int)
                .ok_or(VmError::ArithmeticOverflow),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Float(a - b)),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Multiplies two values of the same numeric kind. Same error conditions
    /// as [`Value::try_add`].
    pub fn try_mul(self, other: Self) -> Result<Self, VmError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a
                .checked_mul(b)
                .map(Self::Int)
                .ok_or(VmError::ArithmeticOverflow),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Float(a * b)),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Divides two values of the same numeric kind.
    ///
    /// # Errors
    /// [`VmError::TypeMismatch`] on mixed/non-numeric types.
    /// [`VmError::DivisionByZero`] for `Int` division by zero. Float division
    /// by zero is not an error here — it follows IEEE 754 (`inf`/`NaN`),
    /// matching `f64`'s own `/` operator.
    pub fn try_div(self, other: Self) -> Result<Self, VmError> {
        match (self, other) {
            (Self::Int(_), Self::Int(0)) => Err(VmError::DivisionByZero),
            (Self::Int(a), Self::Int(b)) => a
                .checked_div(b)
                .map(Self::Int)
                .ok_or(VmError::ArithmeticOverflow),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Float(a / b)),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Negates a numeric value.
    ///
    /// # Errors
    /// [`VmError::TypeMismatch`] on `Bool`. [`VmError::ArithmeticOverflow`]
    /// negating `i64::MIN`.
    pub fn try_neg(self) -> Result<Self, VmError> {
        match self {
            Self::Int(a) => a
                .checked_neg()
                .map(Self::Int)
                .ok_or(VmError::ArithmeticOverflow),
            Self::Float(a) => Ok(Self::Float(-a)),
            Self::Bool(_) => Err(VmError::TypeMismatch),
        }
    }

    /// Logical negation of a `Bool`.
    ///
    /// # Errors
    /// [`VmError::TypeMismatch`] on anything but `Bool`.
    pub fn try_not(self) -> Result<Self, VmError> {
        match self {
            Self::Bool(b) => Ok(Self::Bool(!b)),
            _ => Err(VmError::TypeMismatch),
        }
    }

    /// Equality. Unlike the arithmetic ops, comparing across different kinds
    /// is not an error — it's simply not equal, matching how most stack
    /// languages treat heterogeneous `==`.
    pub fn eq_value(self, other: Self) -> Self {
        Self::Bool(match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            _ => false,
        })
    }

    /// Less-than, for the same numeric kind only.
    ///
    /// # Errors
    /// [`VmError::TypeMismatch`] on `Bool` or mixed `Int`/`Float` operands —
    /// unlike [`Value::eq_value`], ordering across kinds isn't given a
    /// default answer.
    pub fn try_lt(self, other: Self) -> Result<Self, VmError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Bool(a < b)),
            (Self::Float(a), Self::Float(b)) => Ok(Self::Bool(a < b)),
            _ => Err(VmError::TypeMismatch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_on_matching_numeric_kinds() {
        assert_eq!(Value::Int(2).try_add(Value::Int(3)), Ok(Value::Int(5)));
        assert_eq!(
            Value::Float(1.5).try_add(Value::Float(2.5)),
            Ok(Value::Float(4.0))
        );
        assert_eq!(Value::Int(5).try_sub(Value::Int(3)), Ok(Value::Int(2)));
        assert_eq!(Value::Int(4).try_mul(Value::Int(3)), Ok(Value::Int(12)));
        assert_eq!(Value::Int(7).try_div(Value::Int(2)), Ok(Value::Int(3)));
    }

    #[test]
    fn arithmetic_rejects_mixed_or_bool_types() {
        assert_eq!(
            Value::Int(1).try_add(Value::Float(1.0)),
            Err(VmError::TypeMismatch)
        );
        assert_eq!(
            Value::Bool(true).try_add(Value::Int(1)),
            Err(VmError::TypeMismatch)
        );
        assert_eq!(Value::Bool(true).try_neg(), Err(VmError::TypeMismatch));
    }

    #[test]
    fn int_division_by_zero_errors_float_division_does_not() {
        assert_eq!(
            Value::Int(1).try_div(Value::Int(0)),
            Err(VmError::DivisionByZero)
        );
        assert_eq!(
            Value::Float(1.0).try_div(Value::Float(0.0)),
            Ok(Value::Float(f64::INFINITY))
        );
    }

    #[test]
    fn overflow_is_reported_rather_than_wrapping() {
        assert_eq!(
            Value::Int(i64::MAX).try_add(Value::Int(1)),
            Err(VmError::ArithmeticOverflow)
        );
        assert_eq!(
            Value::Int(i64::MIN).try_neg(),
            Err(VmError::ArithmeticOverflow)
        );
    }

    #[test]
    fn eq_value_cross_type_is_false_not_an_error() {
        assert_eq!(
            Value::Int(1).eq_value(Value::Float(1.0)),
            Value::Bool(false)
        );
        assert_eq!(Value::Int(1).eq_value(Value::Int(1)), Value::Bool(true));
        assert_eq!(
            Value::Bool(true).eq_value(Value::Bool(true)),
            Value::Bool(true)
        );
    }

    #[test]
    fn lt_rejects_cross_type_and_bool() {
        assert_eq!(
            Value::Int(1).try_lt(Value::Float(2.0)),
            Err(VmError::TypeMismatch)
        );
        assert_eq!(
            Value::Bool(true).try_lt(Value::Bool(false)),
            Err(VmError::TypeMismatch)
        );
        assert_eq!(Value::Int(1).try_lt(Value::Int(2)), Ok(Value::Bool(true)));
    }

    #[test]
    fn not_rejects_non_bool() {
        assert_eq!(Value::Bool(false).try_not(), Ok(Value::Bool(true)));
        assert_eq!(Value::Int(0).try_not(), Err(VmError::TypeMismatch));
    }
}

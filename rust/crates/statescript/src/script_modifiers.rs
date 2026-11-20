//! Core movement modifier operations and slot keys shared between StateScript and movement.

pub const SPEED_SLOT: u8 = 0;

pub fn narrow(value: f64) -> f64 {
    f64::from(value as f32)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Add,
    Multiply,
    Set,
    Minimum,
    Maximum,
}

impl Operation {
    pub fn apply(self, value: f64, operand: f64) -> f64 {
        match self {
            Self::Add => narrow(value + operand),
            Self::Multiply => narrow(value * operand),
            Self::Set => operand,
            Self::Minimum if operand < value => operand,
            Self::Maximum if operand > value => operand,
            Self::Minimum | Self::Maximum => value,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Modifier {
    pub index: u8,
    pub operation: Operation,
    pub value: f64,
    pub priority: f64,
}

impl Modifier {
    pub fn apply(self, value: f64) -> f64 {
        self.operation.apply(value, self.value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModifierKey {
    pub instance: u16,
    pub state: u16,
    pub slot: usize,
}

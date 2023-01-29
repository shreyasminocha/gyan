use colored::Colorize;
use std::fmt;

use crate::constants::register::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Register {
    A = A as isize,
    B = B as isize,
    C = C as isize,
    D = D as isize,
    S = S as isize,
    I = I as isize,
    F = F as isize,
    None = 0x0,
}

impl Register {
    pub fn try_from(register: u8) -> Option<Register> {
        match register {
            A => Some(Register::A),
            B => Some(Register::B),
            C => Some(Register::C),
            D => Some(Register::D),
            S => Some(Register::S),
            I => Some(Register::I),
            F => Some(Register::F),
            0x0 => Some(Register::None),
            _ => None,
        }
    }

    pub fn to_index(self) -> usize {
        (self as u8).trailing_zeros() as usize
    }
}

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Register::A => write!(f, "{}", "a".red()),
            Register::B => write!(f, "{}", "b".red()),
            Register::C => write!(f, "{}", "c".red()),
            Register::D => write!(f, "{}", "d".red()),
            Register::S => write!(f, "{}", "s".red()),
            Register::I => write!(f, "{}", "i".red()),
            Register::F => write!(f, "{}", "f".red()),
            Register::None => write!(f, "{}", "NONE".black()),
        }
    }
}

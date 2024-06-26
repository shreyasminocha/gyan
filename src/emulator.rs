use std::{
    cmp,
    ffi::CString,
    fs::File,
    io::{Read, Write},
    mem,
    os::fd::{AsRawFd, FromRawFd},
    process::exit,
};

use anyhow::Result;

use crate::yan85::{
    constants::Constants, instruction::Instruction, memory::Memory, register::Register,
    registers::Registers, stack::Stack,
};

/// A Yan85 emulator.
pub struct Emulator {
    /// Encoding constants.
    constants: Constants,
    /// Instructions to emulate.
    instructions: Vec<Instruction>,
    /// The Yan85 registers.
    registers: Registers,
    /// The Yan85 stack.
    stack: Stack,
    /// The Yan85 memory.
    memory: Memory,
}

impl Emulator {
    /// Constructs a new emulator instance.
    pub fn new(constants: Constants, instructions: Vec<Instruction>, memory: Memory) -> Self {
        Self {
            constants,
            instructions,
            registers: Registers::default(),
            stack: Stack::default(),
            memory,
        }
    }

    /// Steps through the next instruction.
    pub fn step(&mut self) -> Result<Instruction> {
        let instruction = self.instructions[self.registers[Register::I] as usize];
        self.registers[Register::I] += 1;

        self.emulate_instruction(instruction)?;

        Ok(instruction)
    }

    /// Emulates a Yan85 instruction.
    fn emulate_instruction(&mut self, instruction: Instruction) -> Result<()> {
        match instruction {
            Instruction::IMM(register, value) => self.emulate_imm(register, value),
            Instruction::ADD(a, b) => self.emulate_add(a, b),
            Instruction::STK(push, pop) => self.emulate_stk(push, pop),
            Instruction::STM(a, b) => self.emulate_stm(a, b),
            Instruction::LDM(a, b) => self.emulate_ldm(a, b),
            Instruction::CMP(a, b) => self.emulate_cmp(a, b),
            Instruction::JMP(condition, register) => self.emulate_jmp(condition, register),
            Instruction::SYS(syscall, register) => self.emulate_sys(syscall, register),
        }
    }

    /// Emulates an `IMM` instruction, assigning `value` to `register`.
    fn emulate_imm(&mut self, register: Register, value: u8) -> Result<()> {
        self.registers[register] = value;
        Ok(())
    }

    /// Emulates an `ADD` instruction, adding the value of `b` to that of `a`, storing the result in
    /// `a`. Overflows wrap around.
    fn emulate_add(&mut self, a: Register, b: Register) -> Result<()> {
        self.registers[a] = self.registers[a].wrapping_add(self.registers[b]);
        Ok(())
    }

    /// Emulates a `STK` instruction, pushing `push`, and popping `pop` unless either
    /// [`Register::None`].
    fn emulate_stk(&mut self, pop: Register, push: Register) -> Result<()> {
        // TODO: handle stack {under,over}flow

        if push != Register::None {
            self.stack[self.registers[Register::S]] = self.registers[push];
            self.registers[Register::S] += 1;
        }

        if pop != Register::None {
            self.registers[Register::S] -= 1;
            self.registers[pop] = self.stack[self.registers[Register::S]];
        }

        Ok(())
    }

    /// Emulates a `STM` instruction, assigning the value of `b` to the location referenced by `a`.
    /// In other words, it performs `*a = b`.
    fn emulate_stm(&mut self, a: Register, b: Register) -> Result<()> {
        self.memory[self.registers[a]] = self.registers[b];
        Ok(())
    }

    /// Emulates a `LDM` instruction, assigning the value at the location referenced by `b` to `a`.
    /// In other words, it performs `a = *b`.
    fn emulate_ldm(&mut self, a: Register, b: Register) -> Result<()> {
        self.registers[a] = self.memory[self.registers[b]];
        Ok(())
    }

    /// Emulates a `CMP` instruction, comparing `a` and `b` and assigning a representation of their
    /// relationship to register F.
    fn emulate_cmp(&mut self, a: Register, b: Register) -> Result<()> {
        let Constants { flag: f, .. } = self.constants;

        let a = self.registers[a];
        let b = self.registers[b];

        let mut flags = 0;

        match a.cmp(&b) {
            cmp::Ordering::Less => flags |= f.L | f.N,
            cmp::Ordering::Greater => flags |= f.G | f.N,
            cmp::Ordering::Equal => flags |= f.E,
        }

        if (a == 0) && (b == 0) {
            flags |= f.Z;
        }

        self.registers[Register::F] = flags;
        Ok(())
    }

    /// Emulates a `JMP` instruction, comparing the conditions encoded in `condition` to those in
    /// register F, jumping to the instruction referenced by `register` if any of the conditions
    /// match.
    fn emulate_jmp(&mut self, condition: u8, register: Register) -> Result<()> {
        if self.registers[Register::F] & condition != 0 {
            self.registers[Register::I] = self.registers[register];
        }

        Ok(())
    }

    /// Emulates a `SYS` instruction, performing a Yan85 system call and placing the return value in
    /// `register`.
    fn emulate_sys(&mut self, syscall: u8, register: Register) -> Result<()> {
        let Constants { syscall: s, .. } = self.constants;

        let a = self.registers[Register::A];
        let b = self.registers[Register::B];
        let c = self.registers[Register::C];

        let return_value = match syscall {
            _ if syscall == s.OPEN => self.syscall_open(a),
            _ if syscall == s.READ_CODE => self.syscall_read_code(a, b, c),
            _ if syscall == s.READ_MEMORY => self.syscall_read_memory(a, b, c),
            _ if syscall == s.WRITE => self.syscall_write(a, b, c),
            _ if syscall == s.SLEEP => self.syscall_sleep(a),
            _ if syscall == s.EXIT => self.syscall_exit(a),
            _ => panic!("unsupported syscall: {syscall:#02x}"),
        };

        self.registers[register] = return_value?;
        Ok(())
    }

    /// Opens the file on the host system with the path pointed to by `path_address`.
    fn syscall_open(&mut self, path_address: u8) -> Result<u8> {
        let path_bytes: Vec<u8> = self.memory[path_address..]
            .iter()
            .take_while(|&&b| b != 0)
            .copied()
            .collect();
        let path = &CString::new(path_bytes).expect("we don't have any null bytes by construction");

        let file = File::open(path.to_str()?)?;
        let fd = file.as_raw_fd();
        mem::forget(file); // don't close the fd upon dropping `file`

        Ok(u8::try_from(fd)?)
    }

    /// Reads up to `num_bytes` bytes from the file with file descriptor `fd` into Yan85
    /// instructions, starting at instruction index `start`.
    fn syscall_read_code(&mut self, fd: u8, start: u8, num_bytes: u8) -> Result<u8> {
        todo!("syscall read_code({fd}, {start:#02x}, {num_bytes:#02x})");
    }

    /// Reads up to `num_bytes` bytes from the file with file descriptor `fd` into memory, starting
    /// at the memory location `start`.
    fn syscall_read_memory(&mut self, fd: u8, start: u8, num_bytes: u8) -> Result<u8> {
        let mut buffer = vec![0u8; num_bytes as usize];

        let mut file = unsafe { File::from_raw_fd(fd.into()) };
        let bytes_read = file.read(&mut buffer)?;
        let bytes_read = u8::try_from(bytes_read).expect("the buffer size is a u8");

        self.memory[start..start + bytes_read].copy_from_slice(&buffer[..bytes_read as usize]);

        Ok(bytes_read)
    }

    /// Writes up to `size` bytes from memory starting at the memory location `start` to the file
    /// with file descriptor `fd`.
    fn syscall_write(&mut self, fd: u8, start: u8, size: u8) -> Result<u8> {
        let bytes_written = unsafe {
            let mut file = File::from_raw_fd(fd.into());
            let n = file.write(&self.memory[start..start + size])?;
            mem::forget(file);

            n
        };

        Ok(u8::try_from(bytes_written).expect("the range size is at most 255"))
    }

    /// Sleeps for `duration` seconds.
    fn syscall_sleep(&mut self, duration: u8) -> Result<u8> {
        todo!("syscall sleep({duration})");
    }

    /// Terminates the Yan85 virtual machine.
    fn syscall_exit(&mut self, exit_code: u8) -> ! {
        exit(exit_code as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_imm() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::IMM(Register::A, 42)],
            Memory::default(),
        );

        emulator.step().unwrap();
        assert_eq!(emulator.registers[Register::A], 42);
    }

    #[test]
    fn test_add() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::ADD(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 42;
        emulator.registers[Register::B] = 24;

        emulator.step().unwrap();

        assert_eq!(emulator.registers[Register::A], 66);
    }

    #[test]
    fn test_stk_push() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::STK(Register::None, Register::C)],
            Memory::default(),
        );

        emulator.registers[Register::C] = 42;

        let sp_pre = emulator.registers[Register::S];
        emulator.step().unwrap();
        let sp_post = emulator.registers[Register::S];

        assert_eq!(sp_post, sp_pre + 1);
        assert_eq!(emulator.stack[emulator.registers[Register::S] - 1], 42);
        assert_eq!(emulator.registers[Register::C], 42);
    }

    #[test]
    fn test_stk_pop() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::STK(Register::B, Register::None)],
            Memory::default(),
        );

        emulator.stack[emulator.registers[Register::S]] = 42;
        emulator.registers[Register::S] += 1;

        let sp_pre = emulator.registers[Register::S];
        emulator.step().unwrap();
        let sp_post = emulator.registers[Register::S];

        assert_eq!(sp_post, sp_pre - 1);
        assert_eq!(emulator.registers[Register::B], 42);
    }

    #[test]
    fn test_stk_push_pop() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::STK(Register::B, Register::C)],
            Memory::default(),
        );

        emulator.registers[Register::C] = 42;

        let sp_pre = emulator.registers[Register::S];
        emulator.step().unwrap();
        let sp_post = emulator.registers[Register::S];

        assert_eq!(sp_post, sp_pre);
        assert_eq!(emulator.registers[Register::B], 42);
        assert_eq!(emulator.registers[Register::C], 42);
    }

    #[test]
    fn test_stm() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::STM(Register::A, Register::D)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 0x20;
        emulator.registers[Register::D] = 42;

        emulator.step().unwrap();
        assert_eq!(emulator.memory[0x20], 42);
    }

    #[test]
    fn test_ldm() {
        let mut emulator = Emulator::new(
            Constants::default(),
            vec![Instruction::LDM(Register::A, Register::D)],
            Memory::default(),
        );

        emulator.memory[0x20] = 42;
        emulator.registers[Register::D] = 0x20;

        emulator.step().unwrap();
        assert_eq!(emulator.registers[Register::A], 42);
    }

    #[test]
    fn test_cmp_less() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![Instruction::CMP(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 1;
        emulator.registers[Register::B] = 2;

        emulator.step().unwrap();

        assert_ne!(emulator.registers[Register::F] & f.L, 0);
        assert_ne!(emulator.registers[Register::F] & f.N, 0);

        assert_eq!(emulator.registers[Register::F] & f.G, 0);
        assert_eq!(emulator.registers[Register::F] & f.E, 0);
        assert_eq!(emulator.registers[Register::F] & f.Z, 0);
    }

    #[test]
    fn test_cmp_greater() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![Instruction::CMP(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 2;
        emulator.registers[Register::B] = 1;

        emulator.step().unwrap();

        assert_ne!(emulator.registers[Register::F] & f.G, 0);
        assert_ne!(emulator.registers[Register::F] & f.N, 0);

        assert_eq!(emulator.registers[Register::F] & f.L, 0);
        assert_eq!(emulator.registers[Register::F] & f.E, 0);
        assert_eq!(emulator.registers[Register::F] & f.Z, 0);
    }

    #[test]
    fn test_cmp_equal() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![Instruction::CMP(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 1;
        emulator.registers[Register::B] = 1;

        emulator.step().unwrap();

        assert_ne!(emulator.registers[Register::F] & f.E, 0);

        assert_eq!(emulator.registers[Register::F] & f.L, 0);
        assert_eq!(emulator.registers[Register::F] & f.G, 0);
        assert_eq!(emulator.registers[Register::F] & f.N, 0);
        assert_eq!(emulator.registers[Register::F] & f.Z, 0);
    }

    #[test]
    fn test_cmp_zeroes() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![Instruction::CMP(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 0;
        emulator.registers[Register::B] = 0;

        emulator.step().unwrap();

        assert_ne!(emulator.registers[Register::F] & f.E, 0);
        assert_ne!(emulator.registers[Register::F] & f.Z, 0);

        assert_eq!(emulator.registers[Register::F] & f.L, 0);
        assert_eq!(emulator.registers[Register::F] & f.G, 0);
        assert_eq!(emulator.registers[Register::F] & f.N, 0);
    }

    #[test]
    fn test_cmp_just_one_zero() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![Instruction::CMP(Register::A, Register::B)],
            Memory::default(),
        );

        emulator.registers[Register::A] = 0;
        emulator.registers[Register::B] = 1;

        emulator.step().unwrap();
        assert_eq!(emulator.registers[Register::F] & f.Z, 0);
    }

    #[test]
    fn test_jmp_taken() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![
                Instruction::JMP(f.L, Register::A),
                Instruction::ADD(Register::C, Register::C),
                Instruction::ADD(Register::C, Register::C),
            ],
            Memory::default(),
        );

        emulator.registers[Register::F] = f.L | f.N;
        emulator.registers[Register::A] = 2;

        emulator.step().unwrap();
        assert_eq!(emulator.registers[Register::I], 2);
    }

    #[test]
    fn test_jmp_not_taken() {
        let consts = Constants::default();
        let Constants { flag: f, .. } = consts;

        let mut emulator = Emulator::new(
            consts,
            vec![
                Instruction::JMP(f.L, Register::A),
                Instruction::ADD(Register::C, Register::C),
                Instruction::ADD(Register::C, Register::C),
            ],
            Memory::default(),
        );

        emulator.registers[Register::F] = f.G | f.N;
        emulator.registers[Register::A] = 2;

        emulator.step().unwrap();

        assert_eq!(emulator.registers[Register::I], 1);
        assert_ne!(emulator.registers[Register::I], 2);
    }

    // TODO: write syscall tests
}

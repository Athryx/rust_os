use crate::uses::*;
use modular_bitfield::{bitfield, BitfieldSpecifier};
use core::ptr;
use crate::int::idt::SPURIOUS;
use crate::int::idt::IRQ_TIMER;
use super::*;

#[bitfield]
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
struct SpuriousReg {
	vec: u8,
	apic_enabled: bool,
	focus_processor_checking: bool,

	#[skip] __: B2,

	suppres_eoi: bool,

	#[skip] __: B19,
}

impl SpuriousReg {
	fn new_enabled(vec: u8) -> Self {
		Self::new().with_vec(vec).with_apic_enabled(true)
	}
}

#[derive(Debug, Clone, Copy, BitfieldSpecifier)]
enum IpiDestShort {
	None = 0,
	This = 1,
	AllExcludeThis = 2,
	AllIncludeThis = 3,
}

#[bitfield]
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub struct CmdReg {
	vector: u8,

	#[bits = 3]
	deliv_mode: DelivMode,

	#[bits = 1]
	dest_mode: DestMode,

	// read only
	#[bits = 1]
	#[skip(setters)]
	status: DelivStatus,

	#[skip] __: B1,

	// true: assert
	// false: de assert
	// should always be true
	assert: bool,

	// should always be IpiTriggerMode::Edge
	#[bits = 1]
	trigger_mode: TriggerMode,

	#[skip] __: B2,

	#[bits = 2]
	dest_short: IpiDestShort,

	#[skip] __: B36,

	dest: u8,
}

impl Default for CmdReg {
	fn default() -> Self {
		Self::new()
			.with_assert(true)
			.with_trigger_mode(TriggerMode::Edge)
	}
}

impl From<Ipi> for CmdReg {
	fn from(ipi: Ipi) -> Self {
		let mut out = Self::default();

		match ipi.dest() {
			IpiDest::This => out.set_dest_short(IpiDestShort::This),
			IpiDest::AllExcludeThis => out.set_dest_short(IpiDestShort::AllExcludeThis),
			IpiDest::AllIncludeThis => out.set_dest_short(IpiDestShort::AllIncludeThis),
			IpiDest::OtherPhysical(dest) => {
				out.set_dest_short(IpiDestShort::None);
				out.set_dest_mode(DestMode::Physical);
				out.set_dest(dest);
			},
			IpiDest::OtherLogical(dest) => {
				out.set_dest_short(IpiDestShort::None);
				out.set_dest_mode(DestMode::Logical);
				out.set_dest(dest);
			},
		}

		match ipi {
			Ipi::To(_, vec) => {
				out.set_vector(vec);
				out.set_deliv_mode(DelivMode::Fixed);
			},
			Ipi::Smi(_) => {
				out.set_vector(0);
				out.set_deliv_mode(DelivMode::Smi);
			},
			Ipi::Init(_) => {
				out.set_vector(0);
				out.set_deliv_mode(DelivMode::Init);
			},
			Ipi::Sipi(_, vec) => {
				out.set_vector(vec);
				out.set_deliv_mode(DelivMode::Sipi);
			},
		}

		out
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpiDest {
	This,
	AllExcludeThis,
	AllIncludeThis,
	OtherPhysical(u8),
	OtherLogical(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipi {
	To(IpiDest, u8),
	Smi(IpiDest),
	Init(IpiDest),
	Sipi(IpiDest, u8),
}

impl Ipi {
	pub fn dest(&self) -> IpiDest {
		match *self {
			Self::To(dest, _) => dest,
			Self::Smi(dest) => dest,
			Self::Init(dest) => dest,
			Self::Sipi(dest, _) => dest,
		}
	}
}

#[derive(Debug, Clone, Copy, BitfieldSpecifier)]
#[bits = 2]
enum LvtTimerMode {
	OneShot = 0,
	Periodic = 1,
	TscDeadline = 2,
}

#[bitfield]
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
struct LvtEntry {
	vec: u8,

	#[bits = 3]
	deliv_mode: DelivMode,

	#[skip] __: B1,

	// read only
	#[bits = 1]
	#[skip(setters)]
	deliv_status: DelivStatus,

	#[bits = 1]
	polarity: PinPolarity,

	// read only
	#[bits = 1]
	#[skip(setters)]
	remote_irr: RemoteIrr,

	#[bits = 1]
	trigger_mode: TriggerMode,

	masked: bool,

	#[bits = 2]
	timer_mode: LvtTimerMode,

	#[skip] __: B13
}

impl LvtEntry {
	fn new_timer(vec: u8) -> Self {
		Self::default()
			.with_timer_mode(LvtTimerMode::Periodic)
			.with_vec(vec)
	}

	fn new_masked() -> Self {
		Self::default().with_masked(true)
	}
}

impl Default for LvtEntry {
	// use default instead of new just in case a flag needs to be set in the future
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Debug, Clone, Copy)]
enum LvtType {
	Timer(LvtEntry),
	MachineCheck(LvtEntry),
	Lint0(LvtEntry),
	Lint1(LvtEntry),
	Error(LvtEntry),
	Perf(LvtEntry),
	Thermal(LvtEntry),
}

impl LvtType {
	fn inner(&self) -> LvtEntry {
		match self {
			Self::Timer(entry) => *entry,
			Self::MachineCheck(entry) => *entry,
			Self::Lint0(entry) => *entry,
			Self::Lint1(entry) => *entry,
			Self::Error(entry) => *entry,
			Self::Perf(entry) => *entry,
			Self::Thermal(entry) => *entry,
		}
	}
}

#[derive(Debug)]
pub struct LocalApic {
	addr: usize,
}

impl LocalApic {
	// offset between registers
	const REG_OFFSET: usize = 0x10;

	const APIC_ID: usize = 0x20;
	const APIC_VERSION: usize = 0x30;

	const TASK_PRIORITY: usize = 0x80;
	const ARBITRATION_PRIORITY: usize = 0x90;
	const PROC_PRIORITY: usize = 0xa0;

	const EOI: usize = 0xb0;

	const REMOTE_READ: usize = 0xc0;

	const LOGICAL_DEST: usize = 0xd0;
	const DEST_FORMAT: usize = 0xe0;

	const SPURIOUS_VEC: usize = 0xf0;

	// 256 bit register
	const IN_SERVICE_BASE: usize = 0x100;

	// 256 bit register
	const TRIGGER_MODE_BASE: usize = 0x180;

	// 256 bit register
	const IRQ_BASE: usize = 0x200;

	const ERROR: usize = 0x280;

	const LVT_MACHINE_CHECK: usize = 0x2f0;
	
	// 64 bit register
	const CMD_BASE: usize = 0x300;

	const LVT_TIMER: usize = 0x320;
	const LVT_THERMAL: usize = 0x330;
	const LVT_PERF: usize = 0x340;
	const LVT_LINT0: usize = 0x350;
	const LVT_LINT1: usize = 0x360;
	const LVT_ERROR: usize = 0x370;

	const TIMER_INIT_COUNT: usize = 0x380;
	const TIMER_COUNT: usize = 0x390;
	const TIMER_DIVIDE_CONFIG: usize = 0x3e0;

	pub fn from(addr: PhysAddr) -> Self {
		let mut out = LocalApic {
			addr: phys_to_virt(addr).as_u64() as usize,
		};
		out.set_lvt(LvtType::Timer(LvtEntry::new_masked()));
		out.set_lvt(LvtType::MachineCheck(LvtEntry::new_masked()));
		out.set_lvt(LvtType::Lint0(LvtEntry::new_masked()));
		out.set_lvt(LvtType::Lint1(LvtEntry::new_masked()));
		// TODO: handle errors
		out.set_lvt(LvtType::Error(LvtEntry::new_masked()));
		out.set_lvt(LvtType::Perf(LvtEntry::new_masked()));
		out.set_lvt(LvtType::Thermal(LvtEntry::new_masked()));
		out.write_reg_32(Self::SPURIOUS_VEC, SpuriousReg::new_enabled(SPURIOUS).into());
		out
	}

	pub fn send_ipi(&mut self, ipi: Ipi) {
		let cmd_reg: CmdReg = ipi.into();
		self.write_reg_64(Self::CMD_BASE, cmd_reg.into())
	}

	pub fn eoi(&mut self) {
		self.write_reg_32(Self::EOI, 0)
	}

	fn set_lvt(&mut self, lvte: LvtType) {
		match lvte {
			LvtType::Timer(entry) => self.write_reg_32(Self::LVT_TIMER, entry.into()),
			LvtType::MachineCheck(entry) => self.write_reg_32(Self::LVT_MACHINE_CHECK, entry.into()),
			LvtType::Lint0(entry) => self.write_reg_32(Self::LVT_LINT0, entry.into()),
			LvtType::Lint1(entry) => self.write_reg_32(Self::LVT_LINT1, entry.into()),
			LvtType::Error(entry) => self.write_reg_32(Self::LVT_ERROR, entry.into()),
			LvtType::Perf(entry) => self.write_reg_32(Self::LVT_PERF, entry.into()),
			LvtType::Thermal(entry) => self.write_reg_32(Self::LVT_THERMAL, entry.into()),
		}
	}

	fn error(&self) -> u32 {
		self.read_reg_32(Self::ERROR)
	}

	fn read_reg_32(&self, reg: usize) -> u32 {
		let ptr = (self.addr + reg) as *const u32;
		unsafe {
			ptr::read_volatile(ptr)
		}
	}

	fn write_reg_32(&mut self, reg: usize, val: u32) {
		let ptr = (self.addr + reg) as *mut u32;
		unsafe {
			ptr::write_volatile(ptr, val);
		}
	}

	fn read_reg_64(&self, reg: usize) -> u64 {
		let high = self.read_reg_32(reg + Self::REG_OFFSET) as u64;
		let low = self.read_reg_32(reg) as u64;

		(high << 32) | low
	}

	// writes bytes in right order for send_ipi
	fn write_reg_64(&mut self, reg: usize, val: u64) {
		let low = get_bits(val as usize, 0..32) as u32;
		let high = get_bits(val as usize, 32..64) as u32;

		self.write_reg_32(reg + Self::REG_OFFSET, high);
		self.write_reg_32(reg, low);
	}

	fn read_reg_256(&self, reg: usize) -> [u64; 4] {
		let mut out = [0; 4];
		for (i, elem) in out.iter_mut().enumerate() {
			*elem = self.read_reg_64(reg + 2 * i * Self::REG_OFFSET);
		}
		out
	}

	fn write_reg_256(&mut self, reg: usize, val: [u64; 4]) {
		for (i, elem) in val.iter().enumerate() {
			self.write_reg_64(reg + 2 * i * Self::REG_OFFSET, *elem);
		}
	}

	/*fn apic_id(&self) -> u32 {
		self.read_reg_32(Self::APIC_ID)
	}

	fn apic_version(&self) -> u32 {
		self.read_reg_32(Self::APIC_VERSION)
	}*/
}

use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::sync::Arc;
use alloc::collections::BTreeMap;

use bitflags::bitflags;

use crate::uses::*;
use crate::sched::FutexMap;
use super::*;
use super::phys_alloc::{zm, Allocation};
use super::virt_alloc::{AllocType, PageMappingFlags, VirtLayout, VirtLayoutElement};

bitflags! {
	pub struct SMemFlags: u8
	{
		const NONE =		0;
		const READ =		1;
		const WRITE =		1 << 1;
		const EXEC =		1 << 2;
	}
}

impl SMemFlags
{
	fn exists(&self) -> bool
	{
		self.intersects(SMemFlags::READ | SMemFlags::WRITE | SMemFlags::EXEC)
	}
}

static next_smid: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct SharedMem
{
	mem: Allocation,
	flags: SMemFlags,
	// this id is not used in any process to reference this shared memory, it is used for scheduler purposes to wait on shared futexes
	id: usize,
	futex: FutexMap,
}

impl SharedMem
{
	pub fn new(size: usize, flags: SMemFlags) -> Option<Arc<Self>>
	{
		let allocation = zm.alloc(size)?;
		let id = next_smid.fetch_add(1, Ordering::Relaxed);
		Some(Arc::new(SharedMem {
			mem: allocation,
			flags,
			id,
			futex: FutexMap::new_smem(id),
		}))
	}

	pub fn id(&self) -> usize
	{
		self.id
	}

	pub fn futex(&self) -> &FutexMap
	{
		&self.futex
	}

	pub fn alloc_type(&self) -> AllocType
	{
		AllocType::Shared(self.id)
	}

	// returns a virtual layout that can be mapped by the virtual memory mapper
	pub fn virt_layout(&self) -> VirtLayout
	{
		let elem = VirtLayoutElement::from_range(
			self.mem.into(),
			PageMappingFlags::from_shared_flags(self.flags),
		);
		VirtLayout::from(vec![elem], self.alloc_type())
	}
}

#[derive(Debug)]
pub struct SMemMapEntry
{
	smem: Arc<SharedMem>,
	pub virt_mem: Option<VirtRange>,
}

impl SMemMapEntry
{
	pub fn smem(&self) -> &Arc<SharedMem>
	{
		&self.smem
	}

	pub fn into_smem(self) -> Arc<SharedMem>
	{
		self.smem
	}
}

#[derive(Debug)]
pub struct SMemMap
{
	data: BTreeMap<usize, SMemMapEntry>,
	next_id: usize,
}

impl SMemMap
{
	pub fn new() -> Self
	{
		SMemMap {
			data: BTreeMap::new(),
			next_id: 0,
		}
	}

	pub fn insert(&mut self, smem: Arc<SharedMem>) -> usize
	{
		let id = self.next_id;
		self.next_id += 1;
		let entry = SMemMapEntry {
			smem,
			virt_mem: None,
		};
		self.data.insert(id, entry);
		id
	}

	pub fn get(&self, id: usize) -> Option<&SMemMapEntry>
	{
		self.data.get(&id)
	}

	pub fn get_mut(&mut self, id: usize) -> Option<&mut SMemMapEntry>
	{
		self.data.get_mut(&id)
	}

	pub fn remove(&mut self, id: usize) -> Option<SMemMapEntry>
	{
		self.data.remove(&id)
	}
}

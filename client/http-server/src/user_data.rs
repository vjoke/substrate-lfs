use codec::Decode;
use frame_support::storage::generator::StorageDoubleMap;
use hyper::Uri;
use pallet_lfs_user_data as pallet;
use sc_client::Client;
use sc_client_api::{backend, CallExecutor};
use sp_core::crypto::Ss58Codec;
use sp_core::storage::StorageKey;
use sp_lfs_core::{LfsId, LfsReference};
use sp_runtime::{generic::BlockId, traits::Block as BlockT};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::traits::Resolver;

#[derive(Clone)]
enum NextResolveStep {
	UserData,
	RootData,
	Glob,
	NotFound,
	End,
}

impl NextResolveStep {
	fn next(&self) -> Self {
		match self {
			NextResolveStep::UserData => NextResolveStep::RootData,
			NextResolveStep::RootData => NextResolveStep::Glob,
			NextResolveStep::Glob => NextResolveStep::NotFound,
			NextResolveStep::NotFound => NextResolveStep::End,
			_ => NextResolveStep::End,
		}
	}
}

pub struct UserDataResolveIterator<L, B, E, Block: BlockT, RA, T: pallet::Trait> {
	client: Arc<Client<B, E, Block, RA>>,
	best_block: BlockId<Block>,
	root_key: T::AccountId,
	_marker: PhantomData<(T, L)>,
	uri: Uri,
	next: NextResolveStep,
}

impl<L, B, E, Block, RA, T> UserDataResolveIterator<L, B, E, Block, RA, T>
where
	B: backend::Backend<Block>,
	E: CallExecutor<Block>,
	Block: BlockT,
	L: LfsId,
	T: pallet::Trait,
{
	fn new(
		client: Arc<Client<B, E, Block, RA>>,
		best_block: BlockId<Block>,
		root_key: T::AccountId,
		uri: Uri,
	) -> Self {
		Self {
			client,
			best_block,
			root_key,
			uri,
			next: NextResolveStep::UserData,
			_marker: Default::default(),
		}
	}

	fn lookup(&self, key: &StorageKey) -> Option<L> {
		self.client
			.storage(&self.best_block, key)
			.map(|o| {
				o.map(|d| {
					// user data is stored as an opaque LFS reference
					LfsReference::decode(&mut d.0.as_slice())
						// which we then convert into an LFSid
						.map(|i| L::try_from(i).ok())
						.map_err(|_| {
							println!("UserData Entry {:?} holds a non-key: {:?}.", key, d.0)
						})
						.ok()
						.flatten()
				})
				.flatten()
			})
			.ok()
			.flatten()
	}
}

impl<L, B, E, Block, RA, T> core::iter::Iterator for UserDataResolveIterator<L, B, E, Block, RA, T>
where
	B: backend::Backend<Block>,
	E: CallExecutor<Block>,
	Block: BlockT,
	L: LfsId,
	T: pallet::Trait,
	T::AccountId: Ss58Codec,
{
	type Item = L;

	fn next(&mut self) -> Option<Self::Item> {
		let mut step = self.next.clone();

		loop {
			let key = match step {
				NextResolveStep::UserData => {
					let mut splitter = self.uri.path().splitn(3, "/");
					let _ = splitter.next(); // it starts with a slash, drop it;
					let user_key = splitter.next();
					user_key
						.map(|mut u| T::AccountId::from_string(&mut u).ok())
						.flatten()
						.map(|key| {
							// the rest is the key we want to look up
							// fallback is to check for `/`
							let path = match splitter.next() {
								Some(s) if s.len() > 0 => s.as_bytes().to_vec(),
								_ => b"/".to_vec(),
							};
							pallet::UserData::<T>::storage_double_map_final_key(&key, path)
						})
				}
				NextResolveStep::RootData => {
					Some(pallet::UserData::<T>::storage_double_map_final_key(
						&self.root_key,
						self.uri.path().split_at(1).1.as_bytes(), // drop leading `/`
					))
				}
				NextResolveStep::Glob => Some(pallet::UserData::<T>::storage_double_map_final_key(
					&self.root_key,
					b".*".to_vec(),
				)),
				NextResolveStep::NotFound => {
					Some(pallet::UserData::<T>::storage_double_map_final_key(
						&self.root_key,
						b"_404".to_vec(),
					))
				}
				NextResolveStep::End => {
					// we are done.
					break;
				}
			};

			step = step.next();

			if let Some(l) = key.map(|k| self.lookup(&StorageKey(k))).flatten() {
				self.next = step;
				return Some(l);
			}
		}

		None
	}
}

/// Resolve uri via on-chain UserData
pub struct UserDataResolver<B, E, Block: BlockT, RA, T> {
	client: Arc<Client<B, E, Block, RA>>,
	_marker: PhantomData<T>,
}

impl<B, E, Block, RA, T> Clone for UserDataResolver<B, E, Block, RA, T>
where
	B: backend::Backend<Block>,
	E: CallExecutor<Block>,
	Block: BlockT,
	T: pallet::Trait,
{
	fn clone(&self) -> Self {
		Self {
			client: self.client.clone(),
			_marker: Default::default(),
		}
	}
}

impl<B, E, Block, RA, T> UserDataResolver<B, E, Block, RA, T>
where
	B: backend::Backend<Block>,
	E: CallExecutor<Block>,
	Block: BlockT,
	T: pallet::Trait,
{
	pub fn new(client: Arc<Client<B, E, Block, RA>>) -> Self {
		UserDataResolver {
			client,
			_marker: Default::default(),
		}
	}
}

impl<B, E, Block, RA, T, L> Resolver<L> for UserDataResolver<B, E, Block, RA, T>
where
	B: backend::Backend<Block>,
	E: CallExecutor<Block>,
	Block: BlockT,
	T: pallet::Trait,
	L: LfsId,
	T::AccountId: Ss58Codec,
{
	/// The iterator this resolves to, must yield `LfsdId`s
	type Iterator = Box<UserDataResolveIterator<L, B, E, Block, RA, T>>;

	/// Given the uri, yield the `LfsId`s to look up
	fn resolve(&self, uri: Uri) -> Option<Self::Iterator> {
		Some(Box::new(UserDataResolveIterator::new(
			self.client.clone(),
			BlockId::Hash(self.client.chain_info().best_hash),
			T::AccountId::default(),
			uri,
		)))
	}
}
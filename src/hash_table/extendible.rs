use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use crate::{
    disk::{Disk, FileSystem},
    hash_table::bucket_page::{Bucket, DEFAULT_BIT_SIZE},
    hash_table::dir_page::{self, Directory},
    page::{PageBuf, PageId},
    page_cache::SharedPageCache,
    storable::Storable,
    writep,
};

// TODO: proper errors
#[derive(Debug)]
pub enum ExtendibleError {
    Error,
}
pub type ExtendibleResult<T> = Result<T, ExtendibleError>;
use ExtendibleError::*;

pub struct ExtendibleHashTable<
    K,
    V,
    D: Disk = FileSystem,
    const BUCKET_BIT_SIZE: usize = DEFAULT_BIT_SIZE,
> {
    dir_page_id: PageId,
    pc: SharedPageCache<D>,
    _data: PhantomData<(K, V)>,
}

impl<const BUCKET_BIT_SIZE: usize, K, V, D> ExtendibleHashTable<K, V, D, BUCKET_BIT_SIZE>
where
    K: Storable + Copy + Eq + Hash,
    V: Storable + Copy + Eq,
    D: Disk,
{
    pub fn new(dir_page_id: PageId, pc: SharedPageCache<D>) -> Self {
        Self {
            dir_page_id,
            pc,
            _data: PhantomData,
        }
    }

    pub async fn insert(&self, k: &K, v: &V) -> ExtendibleResult<bool> {
        let dir_page = self.pc.fetch_page(self.dir_page_id).await.ok_or(Error)?;
        let mut dir_page_w = dir_page.page.write().await;
        let mut dir = Directory::from(&dir_page_w.data);

        let bucket_index = Self::get_bucket_index(k, &dir);
        let bucket_page_id = dir.get(bucket_index);
        let bucket_page = match bucket_page_id {
            0 => {
                let p = self.pc.new_page().await.ok_or(Error)?;
                dir.insert(bucket_index, p.page.read().await.id);
                writep!(dir_page_w, &PageBuf::from(&dir));
                p
            }
            _ => self.pc.fetch_page(bucket_page_id).await.ok_or(Error)?,
        };

        let mut bucket_page_w = bucket_page.page.write().await;
        let mut bucket: Bucket<K, V, BUCKET_BIT_SIZE> = Bucket::from(&bucket_page_w.data);

        bucket.insert(k, v);
        writep!(bucket_page_w, &PageBuf::from(&bucket));

        if bucket.is_full() {
            if dir.local_depth_mask(bucket_index) == dir.global_depth_mask() {
                // The size of the directory implicitily doubles
                dir.incr_global_depth();
            }

            // 1. Create two new bucket pages and increase local depths for both of them
            // 2. Get the high bit of the old bucket (1 << local_depth)
            // 3. Reinsert into the new pages
            // 4. Update the page ids in the directory
            let page0 = self.pc.new_page().await.ok_or(Error)?;
            let mut page0_w = page0.page.write().await;
            let mut bucket0: Bucket<K, V, BUCKET_BIT_SIZE> = Bucket::from(&page0_w.data);

            let page1 = self.pc.new_page().await.ok_or(Error)?;
            let mut page1_w = page1.page.write().await;
            let mut bucket1: Bucket<K, V, BUCKET_BIT_SIZE> = Bucket::from(&page1_w.data);

            let bit = dir.get_local_high_bit(bucket_index);
            for pair in bucket.get_pairs() {
                let i = Self::get_bucket_index(&pair.a, &dir);
                let new_bucket = if i & bit > 0 {
                    &mut bucket1
                } else {
                    &mut bucket0
                };
                new_bucket.insert(&pair.a, &pair.b);
            }

            for i in (Self::get_bucket_index(k, &dir) & (bit - 1)..dir_page::PAGE_IDS_SIZE_U32)
                .step_by(bit)
            {
                let new_page_id = if i & bit > 0 { page0_w.id } else { page1_w.id };

                dir.insert(i, new_page_id);
            }

            writep!(dir_page_w, &PageBuf::from(dir));
            writep!(page0_w, &PageBuf::from(&bucket0));
            writep!(page1_w, &PageBuf::from(&bucket0));

            // TODO: mark original page on disk as ready to be allocated
            self.pc.remove_page(bucket_page_w.id).await;
        }

        Ok(true)
    }

    pub async fn remove(&self, k: &K, v: &V) -> ExtendibleResult<bool> {
        let dir_page = self.pc.fetch_page(self.dir_page_id).await.ok_or(Error)?;
        let dir_page_r = dir_page.page.read().await;
        let dir = Directory::from(&dir_page_r.data);

        let bucket_index = Self::get_bucket_index(k, &dir);
        let bucket_page_id = dir.get(bucket_index);
        let bucket_page = match bucket_page_id {
            0 => return Ok(false),
            _ => self.pc.fetch_page(bucket_page_id).await.ok_or(Error)?,
        };
        let mut bucket_page_w = bucket_page.page.write().await;
        let mut bucket: Bucket<K, V, BUCKET_BIT_SIZE> = Bucket::from(&bucket_page_w.data);

        let ret = bucket.remove(k, v);
        writep!(bucket_page_w, &PageBuf::from(bucket));

        // TODO: attempt to merge if empty

        Ok(ret)
    }

    pub async fn get(&self, k: &K) -> ExtendibleResult<Vec<V>> {
        let dir_page = self.pc.fetch_page(self.dir_page_id).await.ok_or(Error)?;
        let dir_page_r = dir_page.page.read().await;
        let dir = Directory::from(&dir_page_r.data);

        let bucket_index = Self::get_bucket_index(k, &dir);
        let bucket_page_id = dir.get(bucket_index);
        let bucket_page = match bucket_page_id {
            0 => return Ok(vec![]),
            _ => self.pc.fetch_page(bucket_page_id).await.ok_or(Error)?,
        };

        let bucket_page_w = bucket_page.page.read().await;
        let bucket: Bucket<K, V, BUCKET_BIT_SIZE> = Bucket::from(&bucket_page_w.data);

        Ok(bucket.find(k))
    }

    pub async fn get_num_buckets(&self) -> ExtendibleResult<u32> {
        let dir_page = self.pc.fetch_page(self.dir_page_id).await.ok_or(Error)?;
        let dir_page_r = dir_page.page.read().await;
        let dir = Directory::from(&dir_page_r.data);

        Ok(1 << dir.global_depth())
    }

    fn hash(k: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        k.hash(&mut hasher);
        hasher.finish() as usize
    }

    fn get_bucket_index(k: &K, dir_page: &Directory) -> usize {
        let hash = Self::hash(k);
        let i = hash & dir_page.global_depth_mask();

        i % dir_page::PAGE_IDS_SIZE_U32
    }
}

#[cfg(test)]
mod test {
    use crate::{
        disk::FileSystem,
        hash_table::extendible::ExtendibleHashTable,
        hash_table::{bucket_page::DEFAULT_BIT_SIZE, dir_page::Directory},
        page_cache::PageCache,
        replacer::LRUKHandle,
        test::CleanUp,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_extendible_hash_table() {
        let file = "test_extendible_hash_table.db";
        let _cu = CleanUp::file(file);
        let dir_page_id = 0;

        {
            let disk = FileSystem::new(file).await.expect("could not open db file");
            let replacer = LRUKHandle::new(2);
            const POOL_SIZE: usize = 8;
            let pm = PageCache::new(disk, replacer, dir_page_id);
            let _dir_page = pm.new_page().await;
            let ht: ExtendibleHashTable<i32, i32, FileSystem, DEFAULT_BIT_SIZE> =
                ExtendibleHashTable::new(dir_page_id, pm.clone());

            ht.insert(&0, &1).await.unwrap();
            ht.insert(&2, &3).await.unwrap();
            ht.insert(&4, &5).await.unwrap();

            let r1 = ht.get(&0).await.unwrap();
            let r2 = ht.get(&2).await.unwrap();
            let r3 = ht.get(&4).await.unwrap();

            assert!(r1[0] == 1);
            assert!(r2[0] == 3);
            assert!(r3[0] == 5);

            ht.remove(&4, &5).await.unwrap();

            pm.flush_all_pages().await;
        }

        // Make sure it reads back ok
        let disk = FileSystem::new(file).await.expect("could not open db file");
        let replacer = LRUKHandle::new(2);
        let pm = PageCache::new(disk, replacer, dir_page_id + 1);
        let ht: ExtendibleHashTable<i32, i32, FileSystem, DEFAULT_BIT_SIZE> =
            ExtendibleHashTable::new(dir_page_id, pm.clone());

        let r1 = ht.get(&0).await.unwrap();
        let r2 = ht.get(&2).await.unwrap();
        let r3 = ht.get(&4).await.unwrap();

        assert!(r1[0] == 1);
        assert!(r2[0] == 3);
        assert!(r3.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_split() {
        let file = "test_split.db";
        let disk = FileSystem::new(file).await.expect("could not open db file");
        let _cu = CleanUp::file(file);
        // let replacer = LRUKReplacer::new(2);
        let replacer = LRUKHandle::new(2);
        let dir_page_id = 0;
        const POOL_SIZE: usize = 8;
        const BIT_SIZE: usize = 1; // 8 slots
        let pm = PageCache::new(disk, replacer, dir_page_id);
        let _dir_page = pm.new_page().await;
        let ht: ExtendibleHashTable<i32, i32, FileSystem, BIT_SIZE> =
            ExtendibleHashTable::new(dir_page_id, pm.clone());

        assert!(ht.get_num_buckets().await.unwrap() == 1);

        // Global depth should be 1 after this
        ht.insert(&0, &1).await.unwrap();
        ht.insert(&2, &2).await.unwrap();
        ht.insert(&0, &3).await.unwrap();
        ht.insert(&2, &4).await.unwrap();
        ht.insert(&0, &5).await.unwrap();
        ht.insert(&2, &6).await.unwrap();
        ht.insert(&0, &7).await.unwrap();
        ht.insert(&2, &8).await.unwrap();

        assert!(ht.get_num_buckets().await.unwrap() == 2);

        let dir_page = pm.fetch_page(0).await.expect("there should be a page 0");
        let dir_page_w = dir_page.page.write().await;
        let dir = Directory::from(&dir_page_w.data);

        assert!(dir.global_depth() == 1);
    }
}

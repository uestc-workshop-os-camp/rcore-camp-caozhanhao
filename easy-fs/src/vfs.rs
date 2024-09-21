use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    inode_id: u32,
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

pub struct InodeStat {
    pub block_id: u64,
    pub inode_id: u64,
    pub is_dir: bool,
    pub nlink: u32,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        inode_id: u32,
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            inode_id,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id());
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    inode_id,
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create an inode with a given inode id
    fn create_child(&self, name: &str, inode: Option<u32>) -> Option<Arc<Inode>>
    {
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        let is_link: bool;
        let inode_id = if inode.is_none()
        {
            is_link = false;
            fs.alloc_inode()
        } else {
            is_link = true;
            inode.unwrap()
        };
        // initialize inode
        let (inode_block_id, inode_block_offset) = fs.get_disk_inode_pos(inode_id);
        get_block_cache(inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(inode_block_offset, |inode: &mut DiskInode| {
                if is_link { inode.nlink += 1; } else { inode.initialize(DiskInodeType::File); }
            });

        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            inode_id,
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>>
    {
        self.create_child(name, None)
    }

    // TODO: FAILED, WHY
    /// Create inode linked with a given inode name
    // pub fn create_link(&self, name: &str, from: &str) -> Option<Arc<Inode>>
    // {
    //     let inode_id = self.read_disk_inode(|disk_inode| {
    //         self.find_inode_id(from, disk_inode)
    //     });
    //     if inode_id.is_none() {return None;}
    //     self.create_child(name, inode_id)
    // }

    /// Create inode linked with a given inode id
    pub fn create_link_id(&self, name: &str, from_inode_id: u32) -> Option<Arc<Inode>>
    {
        self.create_child(name, Some(from_inode_id))
    }

    /// Destroy linked inode
    pub fn destroy_link(&self, name: &str) -> isize
    {
        let inode_opt = self.find(name);
        if inode_opt.is_none() {return -1;}
        let inode = inode_opt.unwrap();

        let fs = self.fs.lock();
        let (inode_block_id, inode_block_offset) = fs.get_disk_inode_pos(inode.inode_id);
        let nlink = get_block_cache(inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(inode_block_offset, |inode: &mut DiskInode| {
                inode.nlink -= 1;
                inode.nlink
            });
        // clear() will lock self.fs !!!
        drop(fs);
        if nlink == 0
        {
            inode.clear();
        }

        let mut finished = false;
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let mut dirent = DirEntry::empty();
            let empty_dirent = DirEntry::empty();
            for i in 0..file_count {
                assert_eq!(
                    root_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if dirent.name() == name {
                    root_inode.write_at(
                        i * DIRENT_SZ,
                        empty_dirent.as_bytes(),
                        &self.block_device,
                    );
                    finished = true;
                    break;
                }
            }
        });

        assert!(finished);

        block_cache_sync_all();
        0
    }

    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }

    /// Returns the id of the inode
    pub fn id(&self) -> u32
    {
        self.inode_id
    }

    /// Returns the status of the inode
    pub fn stat(&self) -> InodeStat
    {
        let _fs = self.fs.lock();
        let (is_dir, nlink) = self.read_disk_inode(|disk_inode| {
            (disk_inode.is_dir(), disk_inode.nlink)
        });

        InodeStat
        {
            block_id: self.block_id as u64,
            inode_id: self.inode_id as u64,
            is_dir,
            nlink,
        }
    }
}

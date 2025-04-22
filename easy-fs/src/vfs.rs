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
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
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
                return Some(dirent.inode_id() as u32);
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
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
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
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
                new_inode.nlink += 1;
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
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

    /// Link
    pub fn link(&self, old_name: &str, new_name: &str) -> bool{
        let old_inode_id = 
        self.read_disk_inode(|root_node| self.find_inode_id(old_name, root_node));
        if old_inode_id.is_none() {
            return false;
        }
        let mut fs = self.fs.lock();
        let (block_id, block_offset) = fs.get_disk_inode_pos(old_inode_id.unwrap());
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
        .lock()
        .modify(block_offset, |disk_inode: &mut DiskInode| disk_inode.nlink += 1);
        self.modify_disk_inode(|root_node| {
            let file_count = (root_node.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_node, &mut fs);
            let dirent = DirEntry::new(new_name, old_inode_id.unwrap());
            root_node.write_at(file_count * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
        });
        block_cache_sync_all();
        true
    }

    /// Unlink
    pub fn unlink(&self, name: &str) -> bool {
        let mut inode_id: Option<u32> = None;
        let mut v: Vec<DirEntry> = Vec::new();
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    root_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ
                );
                if dirent.name() != name {
                    v.push(dirent);
                } else {
                    inode_id = Some(dirent.inode_id());
                }
            }
        });
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|root_inode| {
            let size = root_inode.size;
            let dealloc_blocks = root_inode.clear_size(&self.block_device);
            assert!(dealloc_blocks.len() == DiskInode::total_blocks(size) as usize);
            for block in dealloc_blocks.into_iter() {
                fs.dealloc_data(block);
            }
            self.increase_size((v.len() * DIRENT_SZ) as u32, root_inode, &mut fs);
            for (i, dirent) in v.iter().enumerate() {
                root_inode.write_at(i * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
            }
        });
        if inode_id.is_none() {
            return false;
        }
        let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id.unwrap());
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
        .lock()
        .modify(block_offset, |disk_inode: &mut DiskInode| {
            disk_inode.nlink -= 1;
            if disk_inode.nlink == 0 {
                let size = disk_inode.size;
                let dealloc_blocks = disk_inode.clear_size(&self.block_device);
                assert!(dealloc_blocks.len() == DiskInode::total_blocks(size) as usize);
                for block_id in dealloc_blocks.into_iter() {
                    fs.dealloc_data(block_id);
                }
            }
        });
        block_cache_sync_all();
        true
    }

    /// inode id
    pub fn get_inode_id(&self) -> u32 {
        let fs = self.fs.lock();
        fs.get_inode_id(self.block_id as u32)
    }
    /// device address
    pub fn get_dev_address(&self) -> *const u8 {
        Arc::as_ptr(&self.block_device) as *const u8
    }
    /// nlink
    pub fn get_nlink(&self) -> u32 {
        self.read_disk_inode(|disk_inode| {
            disk_inode.nlink
        })
    }
    /// is file
    pub fn is_file(&self) -> bool {
        self.read_disk_inode(|disk_inode| {
            disk_inode.is_file()
        })
    }
    /// is dir
    pub fn is_dir(&self) -> bool {
        self.read_disk_inode(|disk_inode| {
            disk_inode.is_dir()
        })
    }    
}

//! Implements the node for /proc/self/exe.
use alloc::{format, sync::Arc};
use axfs_vfs::{VfsNodeAttr, VfsNodeOps, VfsNodeType, VfsResult, VfsDirEntry};
use axtask::{TaskExtRef, current};

use crate::file::resolve_symlink_path;

/// SelfExe 结构体用于表示 /proc/self/exe 的符号链接节点。
/// 该节点用于获取当前进程的可执行文件路径。
pub struct SelfExe;

/// VfsNodeOps trait 的实现，提供符号链接相关操作。
impl VfsNodeOps for SelfExe {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            axfs_vfs::VfsNodePerm::default_file(),
            VfsNodeType::SymLink,
            0,
            0,
        ))
    }

    fn readlink(&self, _path: &str, buf: &mut [u8]) -> VfsResult<usize> {
        let current = current();
        let target = current.task_ext().process_data().exe_path.read();
        let target = resolve_symlink_path(&target);
        let target_bytes = target.as_bytes();
        let copy_len = buf.len().min(target_bytes.len());
        buf[..copy_len].copy_from_slice(&target_bytes[..copy_len]);
        Ok(copy_len)
    }

    fn is_symlink(&self) -> bool {
        true
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}

pub struct SelfFdDir;

impl VfsNodeOps for SelfFdDir {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            axfs_vfs::VfsNodePerm::default_dir(),
            VfsNodeType::Dir,
            0,
            0,
        ))
    }

    fn read_dir(&self, start_idx: usize, vfs_ents: &mut [VfsDirEntry]) -> VfsResult<usize> {
        // 简单实现：返回固定的一些fd条目作为示例
        let sample_fds = [0, 1, 2]; // stdin, stdout, stderr
        let mut count = 0;
        
        for (idx, &fd) in sample_fds.iter().enumerate() {
            if idx >= start_idx && count < vfs_ents.len() {
                let fd_name = format!("{}", fd);
                let name_bytes = fd_name.as_bytes();
                let mut d_name = [0u8; 63];
                let copy_len = name_bytes.len().min(d_name.len() - 1);
                d_name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                vfs_ents[count] = VfsDirEntry::new(&fd_name, VfsNodeType::SymLink);
                count += 1;
            }
        }
        Ok(count)
    }

    fn lookup(self: Arc<SelfFdDir>, name: &str) -> VfsResult<Arc<dyn VfsNodeOps>> {
        // 简单实现：只支持0,1,2这些基本fd
        if let Ok(fd) = name.parse::<usize>() {
            if fd <= 2 {
                return Ok(Arc::new(SelfFdEntry { fd }));
            }
        }
        Err(axfs_vfs::VfsError::NotFound)
    }

    axfs_vfs::impl_vfs_dir_default! {}
}

pub struct SelfFdEntry {
    fd: usize,
}

impl VfsNodeOps for SelfFdEntry {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            axfs_vfs::VfsNodePerm::default_file(),
            VfsNodeType::SymLink,
            0,
            0,
        ))
    }

    fn readlink(&self, _path: &str, buf: &mut [u8]) -> VfsResult<usize> {
        // 简单实现：返回标准流的路径
        let path = match self.fd {
            0 => "/dev/stdin",
            1 => "/dev/stdout", 
            2 => "/dev/stderr",
            _ => "/dev/null",
        };
        let path_bytes = path.as_bytes();
        let copy_len = buf.len().min(path_bytes.len());
        buf[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
        Ok(copy_len)
    }

    fn is_symlink(&self) -> bool {
        true
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}

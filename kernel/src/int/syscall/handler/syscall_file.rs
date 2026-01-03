use crate::{
    int::syscall::SYSCALL_SUCCESS,
    io, memory, multitask, syscall_handler,
    user::handle::{FileHandleObject, HandleObject},
};

syscall_handler! {
    fn syscall_file_create(path_ptr: u64, path_len: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(path_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((path_ptr + path_len) as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let mut path = alloc::vec![0u8; path_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, path_ptr, path.as_mut_ptr(), path_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let filesystem = io::disk::FILE_SYSTEMS.lock().get(&0).cloned().unwrap();
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let Ok(path) = filesystem::path::PathBuf::from_bytes(&path) else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };
            if filesystem.create_file(path.as_path()).await.is_err() {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await; // TODO: 错误类型占位
                return ;
            }
            sender.send(Ok(())).await;
        });

        let result = multitask::async_rt::block_on(receiver.recv()).unwrap();
        match result {
            Ok(()) => SYSCALL_SUCCESS,
            Err(error) => error,
        }
    }
}

syscall_handler! {
    fn syscall_file_open(path_ptr: u64, path_len: u64, handle_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(path_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((path_ptr + path_len) as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let mut path = alloc::vec![0u8; path_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, path_ptr, path.as_mut_ptr(), path_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let filesystem = io::disk::FILE_SYSTEMS.lock().get(&0).cloned().unwrap();
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let Ok(path) = filesystem::path::PathBuf::from_bytes(&path) else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };
            let Ok(handle) = filesystem.open_file(path.as_path()).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await; // TODO: 错误类型占位
                return ;
            };
            sender.send(Ok(handle)).await;
        });

        let handle = multitask::async_rt::block_on(receiver.recv()).unwrap();
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => return error,
        };

        let file_handle = FileHandleObject::new(handle);
        let handle = multitask::process::insert_process_handle(&process, HandleObject::File(file_handle)) as u64;

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, handle_ptr, &handle).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_read(handle: u64, buffer_ptr: u64, buffer_len: u64, read_count_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(buffer_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((buffer_ptr + buffer_len) as usize) ||
            !memory::page::is_user_space_virtual_memory(read_count_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut buffer = alloc::vec![0u8; buffer_len as usize];
        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.read(&mut buffer).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok((count, buffer))).await;
        });
        let (read_count, buffer) = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(read) => read,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory(&process, buffer_ptr, buffer.as_ptr(), read_count as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
            if multitask::process::write_user_process_memory_struct(&process, read_count_ptr, &read_count).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_write(handle: u64, buffer_ptr: u64, buffer_len: u64, write_count_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(buffer_ptr as usize) ||
            !memory::page::is_user_space_virtual_memory((buffer_ptr + buffer_len) as usize) ||
            !memory::page::is_user_space_virtual_memory(write_count_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let mut buffer = alloc::vec![0u8; buffer_len as usize];
        unsafe {
            if multitask::process::read_user_process_memory(&process, buffer_ptr, buffer.as_mut_ptr(), buffer_len as usize).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.write(&buffer).await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(count)).await;
        });
        let read_count = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(read_count) => read_count,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, write_count_ptr, &read_count).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_close(handle: u64) {
        let process = multitask::process::current_process().unwrap();

        multitask::process::remove_process_handle(&process, handle as usize);
    }
}

syscall_handler! {
    fn syscall_file_get_pos(handle: u64, pos_ptr: u64) -> u64 {
        if !memory::page::is_user_space_virtual_memory(pos_ptr as usize) {
            return cos_sys::error::ErrorKind::SegmentationFault as u64;
        }

        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            let Ok(count) = file.get_pointer().await else {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(count)).await;
        });
        let pos = match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(pos) => pos,
            Err(error) => return error,
        };

        unsafe {
            if multitask::process::write_user_process_memory_struct(&process, pos_ptr, &pos).is_err() {
                return cos_sys::error::ErrorKind::SegmentationFault as u64;
            }
        }

        SYSCALL_SUCCESS
    }
}

syscall_handler! {
    fn syscall_file_set_pos(handle: u64, pos: u64) -> u64 {
        let process = multitask::process::current_process().unwrap();

        let Some(handle) = multitask::process::get_process_handle(&process, handle as usize) else {
            return cos_sys::error::ErrorKind::BadArgument as u64;
        };

        let (sender, receiver) = async_locks::channel::oneshot::channel();
        multitask::async_rt::spawn(async move {
            let HandleObject::File(handle) = &*handle else {
                sender.send(Err(cos_sys::error::ErrorKind::BadArgument as u64)).await;
                return;
            };

            let mut file = handle.lock().await;
            if file.move_pointer(pos).await.is_err() {
                sender.send(Err(cos_sys::error::ErrorKind::Unknown as u64)).await;
                return;
            };
            sender.send(Ok(())).await;
        });
        match multitask::async_rt::block_on(receiver.recv()).unwrap() {
            Ok(()) => SYSCALL_SUCCESS,
            Err(error) => error,
        }
    }
}

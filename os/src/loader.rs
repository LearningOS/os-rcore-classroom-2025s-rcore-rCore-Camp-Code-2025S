//! Loading user applications into memory

/// Get the total number of applications.
use alloc::vec::Vec;
use lazy_static::*;
///get app number
pub fn get_num_app() -> usize {
    //获取链接到内核内的应用的数目
    extern "C" {
        fn _num_app();
    }
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}
/// get applications data
pub fn get_app_data(app_id: usize) -> &'static [u8] {
    //返回第 app_id 个应用程序的二进制数据（
    extern "C" {
        fn _num_app();
    }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app(); //获取应用总数
    let app_start = unsafe { 
        core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1)
        //raed应用的所有地址范围
    };
    assert!(app_id < num_app); //检查合法
    unsafe {
        core::slice::from_raw_parts(
            app_start[app_id] as *const u8,  //目标应用的起始地址
            app_start[app_id + 1] - app_start[app_id], //计算长度
        )
    }
}

lazy_static! {
    ///All of app's name
    static ref APP_NAMES: Vec<&'static str> = {
        let num_app = get_num_app();
        extern "C" {
            fn _app_names();
        }
        let mut start = _app_names as usize as *const u8;
        let mut v = Vec::new();
        unsafe {
            for _ in 0..num_app {
                let mut end = start;
                while end.read_volatile() != b'\0' {
                    end = end.add(1);
                }
                let slice = core::slice::from_raw_parts(start, end as usize - start as usize);
                let str = core::str::from_utf8(slice).unwrap();
                v.push(str);
                start = end.add(1);
            }
        }
        v
    };
}

#[allow(unused)]
///get app data from name
pub fn get_app_data_by_name(name: &str) -> Option<&'static [u8]> {
    // 三查找获得应用的 ELF 数据
    let num_app = get_num_app();
    (0..num_app)
        .find(|&i| APP_NAMES[i] == name)
        .map(get_app_data)
}
///list all apps
pub fn list_apps() {
    // 打印出所有可用应用的名字
    println!("/**** APPS ****");
    for app in APP_NAMES.iter() {
        println!("{}", app);
    }
    println!("**************/");
}

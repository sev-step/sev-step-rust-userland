use std::{mem, num::NonZeroUsize};

use anyhow::{bail, Context, Result};
use iced_x86::{code_asm::*, Instruction};
use nix::sys::mman::{self, MapFlags, ProtFlags};
use vm_server::req_resp::{InitAssemblyTargetReq, InitAssemblyTargetResp};

use std::ptr;

//mode 1:
//client generates assembly
//sends assembly to server
//server maps it page aligned and executes it
//client can specify how much memory he wants to get allocated that is passed to his function as a single argument
//memory is guaranteed to by page aligned

//mode 2:
//client sends binary
//server copies binary to file sytems and executes it
//allow caching by first comparing the hash before sending it

//common behaviour
//three phases
//1. prepare : server allocates resources and sends back information struct
//2. run : server starts the binary
//3. cleanup : server frees resources
#[allow(dead_code)]
fn serialize_deserialize_run() -> Result<()> {
    let mut a = CodeAssembler::new(64)?;

    a.nop()?;
    a.nop()?;
    a.ret()?;

    let required_bytes = a
        .assemble(0x0)
        .context("failed to assemble with dummy start addr")?
        .len();

    println!("Assembled code requires {} bytes", required_bytes);

    let executeable_buffer;
    unsafe {
        executeable_buffer = mman::mmap(
            None,
            NonZeroUsize::new_unchecked(4096),
            ProtFlags::all(),
            MapFlags::MAP_ANON | MapFlags::MAP_PRIVATE,
            -1,
            0,
        )?;
    }

    /*let code =a.assemble(executeable_buffer as u64)?;
    println!("Final assembled code {:x?}",code);
    unsafe {ptr::copy(code.as_ptr(), executeable_buffer as *mut u8, code.len())};
    //this makes rust call the code with c calling conventions. Thus we are responsible that our code adheres to these conventions
    let as_c_func: unsafe extern "C" fn() = unsafe {mem::transmute(executeable_buffer)};
    unsafe {as_c_func()};*/

    let instrs = a.take_instructions();

    let seriaized = serde_json::to_string(&instrs).unwrap();
    println!("Serialized instructions {}", seriaized);

    let received_instrs: Vec<Instruction> = serde_json::from_str(&seriaized)?;

    let mut b = CodeAssembler::new(64)?;
    for x in received_instrs {
        b.add_instruction(x)?;
    }

    let code = b.assemble(executeable_buffer as u64)?;
    println!("Final assembled code {:x?}", code);
    unsafe { ptr::copy(code.as_ptr(), executeable_buffer as *mut u8, code.len()) };
    //this makes rust call the code with c calling conventions. Thus we are responsible that our code adheres to these conventions
    let as_c_func: unsafe extern "C" fn() = unsafe { mem::transmute(executeable_buffer) };
    unsafe { as_c_func() };
    Ok(())
}

fn main() -> Result<()> {
    /*let mut a = CodeAssembler::new(64)?;
    a.mov(rax, 1_u64)?;
    a.ret()?;

    };*/

    let mut a = CodeAssembler::new(64)?;
    let data_buffer_size = 128;
    for _off in (0..data_buffer_size).step_by(64) {
        a.mov(rsi, qword_ptr(rdi))
            .context(format!("failed add add mov for mem offset 0x{:x}", _off))?;
        a.add(rdi, 64)
            .context(format!("failed to add add to rdi at _off=0x{:x}", _off))?;
    }
    a.ret()?;

    let req = InitAssemblyTargetReq {
        code: a.take_instructions(),
        required_mem_bytes: 0,
    };

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("http://localhost:3000/assembly-target/new")
        .json(&req)
        .send()
        .context("failed to send \"new\" request")?;

    let assembly_target_resp;
    if !resp.status().is_success() {
        bail!("Server returned error {}", resp.text()?)
    } else {
        assembly_target_resp = resp
            .json::<InitAssemblyTargetResp>()
            .context("failed to decode response to \"new\" request to InitAssemblyTargetResp")?;
    }
    println!("Response to \"new\" request: {}", assembly_target_resp);

    client
        .post("http://localhost:3000/run-target")
        .send()
        .context("failed to send \"run\", request")?;
    println!("Done");

    Ok(())
}

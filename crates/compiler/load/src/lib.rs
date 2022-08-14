use bumpalo::Bump;
use roc_can::module::ExposedByModule;
use roc_collections::all::MutMap;
use roc_module::symbol::{ModuleId, Symbol};
use roc_reporting::report::RenderTarget;
use roc_target::TargetInfo;
use roc_types::subs::{Subs, Variable};
use std::path::PathBuf;

const SKIP_SUBS_CACHE: bool = {
    match option_env!("ROC_SKIP_SUBS_CACHE") {
        Some(s) => s.len() == 1 && s.as_bytes()[0] == b'1',
        None => false,
    }
};

pub use roc_load_internal::docs;
pub use roc_load_internal::file::{
    EntryPoint, ExecutionMode, Expectations, LoadConfig, LoadResult, LoadStart, LoadedModule,
    LoadingProblem, MonomorphizedModule, Phase, Threading,
};

#[allow(clippy::too_many_arguments)]
fn load<'a>(
    arena: &'a Bump,
    load_start: LoadStart<'a>,
    exposed_types: ExposedByModule,
    load_config: LoadConfig,
) -> Result<LoadResult<'a>, LoadingProblem<'a>> {
    let cached_subs = read_cached_subs();

    roc_load_internal::file::load(arena, load_start, exposed_types, cached_subs, load_config)
}

/// Load using only a single thread; used when compiling to webassembly
pub fn load_single_threaded<'a>(
    arena: &'a Bump,
    load_start: LoadStart<'a>,
    exposed_types: ExposedByModule,
    target_info: TargetInfo,
    render: RenderTarget,
    exec_mode: ExecutionMode,
) -> Result<LoadResult<'a>, LoadingProblem<'a>> {
    let cached_subs = read_cached_subs();

    roc_load_internal::file::load_single_threaded(
        arena,
        load_start,
        exposed_types,
        target_info,
        cached_subs,
        render,
        exec_mode,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn load_and_monomorphize_from_str<'a>(
    arena: &'a Bump,
    filename: PathBuf,
    src: &'a str,
    src_dir: PathBuf,
    exposed_types: ExposedByModule,
    load_config: LoadConfig,
) -> Result<MonomorphizedModule<'a>, LoadingProblem<'a>> {
    use LoadResult::*;

    let load_start = LoadStart::from_str(arena, filename, src, src_dir)?;

    match load(arena, load_start, exposed_types, load_config)? {
        Monomorphized(module) => Ok(module),
        TypeChecked(_) => unreachable!(""),
    }
}

pub fn load_and_monomorphize(
    arena: &Bump,
    filename: PathBuf,
    exposed_types: ExposedByModule,
    load_config: LoadConfig,
) -> Result<MonomorphizedModule<'_>, LoadingProblem<'_>> {
    use LoadResult::*;

    let load_start = LoadStart::from_path(arena, filename, load_config.render)?;

    match load(arena, load_start, exposed_types, load_config)? {
        Monomorphized(module) => Ok(module),
        TypeChecked(_) => unreachable!(""),
    }
}

pub fn load_and_typecheck(
    arena: &Bump,
    filename: PathBuf,
    exposed_types: ExposedByModule,
    load_config: LoadConfig,
) -> Result<LoadedModule, LoadingProblem<'_>> {
    use LoadResult::*;

    let load_start = LoadStart::from_path(arena, filename, load_config.render)?;

    match load(arena, load_start, exposed_types, load_config)? {
        Monomorphized(_) => unreachable!(""),
        TypeChecked(module) => Ok(module),
    }
}

pub fn load_and_typecheck_str<'a>(
    arena: &'a Bump,
    filename: PathBuf,
    source: &'a str,
    src_dir: PathBuf,
    exposed_types: ExposedByModule,
    target_info: TargetInfo,
    render: RenderTarget,
) -> Result<LoadedModule, LoadingProblem<'a>> {
    use LoadResult::*;

    let load_start = LoadStart::from_str(arena, filename, source, src_dir)?;

    // NOTE: this function is meant for tests, and so we use single-threaded
    // solving so we don't use too many threads per-test. That gives higher
    // throughput for the test run overall
    match load_single_threaded(
        arena,
        load_start,
        exposed_types,
        target_info,
        render,
        ExecutionMode::Check,
    )? {
        Monomorphized(_) => unreachable!(""),
        TypeChecked(module) => Ok(module),
    }
}

fn deserialize_help(bytes: &[u8]) -> (Subs, Vec<(Symbol, Variable)>) {
    let (subs, slice) = Subs::deserialize(bytes);

    (subs, slice.to_vec())
}

fn read_cached_subs() -> MutMap<ModuleId, (Subs, Vec<(Symbol, Variable)>)> {
    let mut output = MutMap::default();

    // Wasm seems to re-order definitions between build time and runtime, but only in release mode.
    // That is very strange, but we can solve it separately
    #[cfg(not(windows))]
    if !cfg!(target_family = "wasm") && !SKIP_SUBS_CACHE {
        const BOOL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Bool.dat")) as &[_];
        const RESULT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Result.dat")) as &[_];
        const LIST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/List.dat")) as &[_];
        const STR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Str.dat")) as &[_];
        const DICT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Dict.dat")) as &[_];
        const SET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Set.dat")) as &[_];
        const BOX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Box.dat")) as &[_];
        const NUM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/Num.dat")) as &[_];

        output.insert(ModuleId::BOOL, deserialize_help(BOOL));
        output.insert(ModuleId::RESULT, deserialize_help(RESULT));
        output.insert(ModuleId::NUM, deserialize_help(NUM));

        output.insert(ModuleId::LIST, deserialize_help(LIST));
        output.insert(ModuleId::STR, deserialize_help(STR));
        output.insert(ModuleId::DICT, deserialize_help(DICT));

        output.insert(ModuleId::SET, deserialize_help(SET));
        output.insert(ModuleId::BOX, deserialize_help(BOX));
    }

    output
}

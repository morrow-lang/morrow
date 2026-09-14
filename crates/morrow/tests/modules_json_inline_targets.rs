use morrow_compiler::{check, modules};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "morrow-inline-codec-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn inline_targets_resolve_selected_reexports_in_the_type_namespace() {
    let project = Project::new();
    project.write("model.fn","pub type Box(a) derive(Json):\n    value:a\npub type Data=Box(Int)\npub fn Data()->Int:42\n");
    project.write("api.fn", "pub import model.{Box,Data}\n");
    for import in ["import api as m", "import api.{Box,Data}", "import api.*"] {
        let prefix = if import.ends_with("as m") { "m." } else { "" };
        let source = format!(
            "{import}\nfn read(text:String):json.decode(text,{prefix}Box(Int) | String)\nfn other(text:String):json.decode(text,List({prefix}Data | Bool))\nfn main():()\n"
        );
        let loaded = modules::load(&project.write("main.fn", &source)).unwrap();
        check::check(&loaded.program).unwrap();
    }
}
#[test]
fn inline_syntax_never_promotes_private_types_or_fake_codec_aliases() {
    let project = Project::new();
    project.write("model.fn","type Hidden derive(Json):\n    value:Int\npub fn Hidden()->Int:1\npub fn decode(text:String,value:Int)->Int:value\n");
    for source in [
        "import model as m\nfn read(text:String):json.decode(text,m.Hidden | String)\nfn main():()\n",
        "import model as json\nfn read(text:String):json.decode(text,Int | String)\nfn main():()\n",
        "import model.{decode}\nfn read(text:String):decode(text,Int | String)\nfn main():()\n",
    ] {
        let result = modules::load(&project.write("main.fn", source));
        assert!(
            result
                .map(|loaded| check::check(&loaded.program).is_err())
                .unwrap_or(true),
            "{source}"
        );
    }
}

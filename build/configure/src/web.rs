// Copyright: Ankitects Pty Ltd and contributors
// License: GNU AGPL, version 3 or later; http://www.gnu.org/licenses/agpl.html

use anyhow::Result;
use ninja_gen::action::BuildAction;
use ninja_gen::copy::CopyFiles;
use ninja_gen::glob;
use ninja_gen::hashmap;
use ninja_gen::input::BuildInput;
use ninja_gen::inputs;
use ninja_gen::node::node_archive;
use ninja_gen::node::CompileSass;
use ninja_gen::node::DPrint;
use ninja_gen::node::EsbuildScript;
use ninja_gen::node::Eslint;
use ninja_gen::node::GenTypescriptProto;
use ninja_gen::node::Prettier;
use ninja_gen::node::SqlFormat;
use ninja_gen::node::SvelteCheck;
use ninja_gen::node::SveltekitBuild;
use ninja_gen::node::ViteTest;
use ninja_gen::rsync::RsyncFiles;
use ninja_gen::Build;

pub fn build_and_check_web(build: &mut Build) -> Result<()> {
    setup_node(build)?;
    build_sass(build)?;
    build_and_check_tslib(build)?;
    build_sveltekit(build)?;
    declare_and_check_other_libraries(build)?;
    build_and_check_pages(build)?;
    build_and_check_editor(build)?;
    build_and_check_reviewer(build)?;
    build_and_check_mathjax(build)?;
    check_web(build)?;

    Ok(())
}

fn build_sveltekit(build: &mut Build) -> Result<()> {
    build.add_action(
        "sveltekit",
        SveltekitBuild {
            output_folder: inputs!["sveltekit"],
            deps: inputs![
                "ts/tsconfig.json",
                glob!["ts/**", "ts/.svelte-kit/**"],
                ":ts:lib"
            ],
        },
    )
}

fn setup_node(build: &mut Build) -> Result<()> {
    ninja_gen::node::setup_node(
        build,
        node_archive(build.host_platform),
        &[
            "dprint",
            "svelte-check",
            "eslint",
            "sass",
            "tsc",
            "tsx",
            "vite",
            "vitest",
            "protoc-gen-es",
            "prettier",
        ],
        hashmap! {
            "jquery" => vec![
                "jquery/dist/jquery.min.js".into()
            ],
            "jquery-ui" => vec![
                "jquery-ui-dist/jquery-ui.min.js".into()
            ],
            "bootstrap-dist" => vec![
                "bootstrap/dist/js/bootstrap.bundle.min.js".into(),
            ],
            "mathjax" => MATHJAX_FILES.iter().map(|&v| v.into()).collect(),
            "mdi_unthemed" => [
                // saved searches
                "heart-outline.svg",
                // today
                "clock-outline.svg",
                // state
                "circle.svg",
                "circle-outline.svg",
                // flags
                "flag-variant.svg",
                "flag-variant-outline.svg",
                "flag-variant-off-outline.svg",
                // decks
                "book-outline.svg",
                "book-clock-outline.svg",
                "book-cog-outline.svg",
                // notetypes
                "newspaper.svg",
                // cardtype
                "application-braces-outline.svg",
                // fields
                "form-textbox.svg",
                // tags
                "tag-outline.svg",
                "tag-off-outline.svg",
            ].iter().map(|file| format!("@mdi/svg/svg/{file}").into()).collect(),
            "mdi_themed" => [
                // sidebar tools
                "magnify.svg",
                "selection-drag.svg",
                // QComboBox arrows
                "chevron-up.svg",
                "chevron-down.svg",
                // QHeaderView arrows
                "menu-up.svg",
                "menu-down.svg",
                // drag handle
                "drag-vertical.svg",
                "drag-horizontal.svg",
                // checkbox
                "check.svg",
                "minus-thick.svg",
                // QRadioButton
                "circle-medium.svg",
            ].iter().map(|file| format!("@mdi/svg/svg/{file}").into()).collect(),
        },
    )?;
    Ok(())
}

fn build_and_check_tslib(build: &mut Build) -> Result<()> {
    build.add_dependency("ts:generated:i18n", ":rslib:i18n:ts".into());
    build.add_action(
        "ts:generated:proto",
        GenTypescriptProto {
            protos: inputs![glob!["proto/**/*.proto"]],
            include_dirs: &["proto"],
            out_dir: "out/ts/lib/generated",
            out_path_transform: |path| {
                path.replace("proto/", "ts/lib/generated/")
                    .replace("proto\\", "ts/lib/generated\\")
            },
            ts_transform_script: "ts/tools/markpure.ts",
        },
    )?;
    // ensure _service files are generated by rslib
    build.add_dependency("ts:generated:proto", inputs![":rslib:proto:ts"]);
    // copy source files from ts/lib/generated
    build.add_action(
        "ts:generated:src",
        CopyFiles {
            inputs: inputs![glob!["ts/lib/generated/*.ts"]],
            output_folder: "ts/lib/generated",
        },
    )?;

    let src_files = inputs![glob!["ts/lib/**"]];

    build.add_dependency("ts:lib", inputs![":ts:generated"]);
    build.add_dependency("ts:lib", src_files);

    Ok(())
}

fn declare_and_check_other_libraries(build: &mut Build) -> Result<()> {
    for (library, inputs) in [
        ("sveltelib", inputs![":ts:lib", glob!("ts/sveltelib/**")]),
        ("domlib", inputs![":ts:lib", glob!("ts/domlib/**")]),
        (
            "components",
            inputs![":ts:lib", ":ts:sveltelib", glob!("ts/components/**")],
        ),
        ("html-filter", inputs![glob!("ts/html-filter/**")]),
    ] {
        let library_with_ts = format!("ts:{library}");
        build.add_dependency(&library_with_ts, inputs.clone());
    }

    Ok(())
}

fn build_and_check_pages(build: &mut Build) -> Result<()> {
    let mut build_page = |name: &str, html: bool, deps: BuildInput| -> Result<()> {
        let group = format!("ts:{name}");
        let deps = inputs![deps, glob!(format!("ts/{name}/**"))];
        let extra_exts = if html { &["css", "html"][..] } else { &["css"] };
        let entrypoint = if html {
            format!("ts/routes/{name}/index.ts")
        } else {
            format!("ts/{name}/index.ts")
        };
        build.add_action(
            &group,
            EsbuildScript {
                script: inputs!["ts/bundle_svelte.mjs"],
                entrypoint: inputs![entrypoint],
                output_stem: &format!("ts/{name}/{name}"),
                deps: deps.clone(),
                extra_exts,
            },
        )?;
        build.add_dependency("ts:pages", inputs![format!(":{group}")]);

        Ok(())
    };
    // we use the generated .css file separately
    build_page(
        "editable",
        false,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:domlib",
            ":ts:sveltelib",
            ":sass",
            ":sveltekit",
        ],
    )?;
    build_page(
        "congrats",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":sass",
            ":sveltekit"
        ],
    )?;

    Ok(())
}

fn build_and_check_editor(build: &mut Build) -> Result<()> {
    let editor_deps = inputs![
        //
        ":ts:lib",
        ":ts:components",
        ":ts:domlib",
        ":ts:sveltelib",
        ":ts:html-filter",
        ":sass",
        ":sveltekit",
        glob!("ts/{editable,editor,routes/image-occlusion}/**")
    ];

    build.add_action(
        "ts:editor",
        EsbuildScript {
            script: "ts/bundle_svelte.mjs".into(),
            entrypoint: "ts/editor/index.ts".into(),
            output_stem: "ts/editor/editor",
            deps: editor_deps.clone(),
            extra_exts: &["css"],
        },
    )?;

    Ok(())
}

fn build_and_check_reviewer(build: &mut Build) -> Result<()> {
    let reviewer_deps = inputs![
        ":ts:lib",
        glob!("ts/{reviewer,image-occlusion}/**"),
        ":sveltekit"
    ];
    build.add_action(
        "ts:reviewer:reviewer.js",
        EsbuildScript {
            script: inputs!["ts/bundle_ts.mjs"],
            entrypoint: "ts/reviewer/index_wrapper.ts".into(),
            output_stem: "ts/reviewer/reviewer",
            deps: reviewer_deps.clone(),
            extra_exts: &[],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer.css",
        CompileSass {
            input: inputs!["ts/reviewer/reviewer.scss"],
            output: "ts/reviewer/reviewer.css",
            deps: inputs![":sass", "ts/routes/image-occlusion/review.scss"],
            load_paths: vec!["."],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer_extras_bundle.js",
        EsbuildScript {
            script: inputs!["ts/bundle_ts.mjs"],
            entrypoint: "ts/reviewer/reviewer_extras.ts".into(),
            output_stem: "ts/reviewer/reviewer_extras_bundle",
            deps: reviewer_deps.clone(),
            extra_exts: &[],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer_extras.css",
        CompileSass {
            input: inputs!["ts/reviewer/reviewer_extras.scss"],
            output: "ts/reviewer/reviewer_extras.css",
            deps: inputs!["ts/routes/image-occlusion/review.scss"],
            load_paths: vec!["."],
        },
    )?;

    Ok(())
}

fn check_web(build: &mut Build) -> Result<()> {
    let fmt_excluded = "{target,ts/.svelte-kit,node_modules}/**";
    let dprint_files = inputs![glob!["**/*.{ts,mjs,js,md,json,toml,scss}", fmt_excluded]];
    let prettier_files = inputs![glob!["**/*.svelte", fmt_excluded]];

    build.add_action(
        "check:format:dprint",
        DPrint {
            inputs: dprint_files.clone(),
            check_only: true,
        },
    )?;
    build.add_action(
        "format:dprint",
        DPrint {
            inputs: dprint_files,
            check_only: false,
        },
    )?;
    build.add_action(
        "check:format:prettier",
        Prettier {
            inputs: prettier_files.clone(),
            check_only: true,
        },
    )?;
    build.add_action(
        "format:prettier",
        Prettier {
            inputs: prettier_files,
            check_only: false,
        },
    )?;
    build.add_action(
        "check:vitest",
        ViteTest {
            deps: inputs![
                ":node_modules",
                ":ts:generated",
                glob!["ts/{svelte.config.js,vite.config.ts,tsconfig.json}"],
                glob!["ts/{lib,deck-options,html-filter,domlib,reviewer,change-notetype}/**/*"],
            ],
        },
    )?;
    build.add_action(
        "check:svelte",
        SvelteCheck {
            tsconfig: inputs!["ts/tsconfig.json"],
            inputs: inputs![
                ":node_modules",
                ":ts:generated",
                glob!["ts/**/*", "ts/.svelte-kit/**"],
            ],
        },
    )?;
    let eslint_rc = inputs![".eslintrc.cjs"];
    for folder in ["ts", "qt/aqt/data/web/js"] {
        let inputs = inputs![glob![format!("{folder}/**"), "ts/.svelte-kit/**"]];
        build.add_action(
            "check:eslint",
            Eslint {
                folder,
                inputs: inputs.clone(),
                eslint_rc: eslint_rc.clone(),
                fix: false,
            },
        )?;
        build.add_action(
            "fix:eslint",
            Eslint {
                folder,
                inputs,
                eslint_rc: eslint_rc.clone(),
                fix: true,
            },
        )?;
    }

    Ok(())
}

pub fn check_sql(build: &mut Build) -> Result<()> {
    build.add_action(
        "check:format:sql",
        SqlFormat {
            inputs: inputs![glob!["**/*.sql"]],
            check_only: true,
        },
    )?;
    build.add_action(
        "format:sql",
        SqlFormat {
            inputs: inputs![glob!["**/*.sql"]],
            check_only: false,
        },
    )?;
    Ok(())
}

fn build_and_check_mathjax(build: &mut Build) -> Result<()> {
    let files = inputs![glob!["ts/mathjax/*"], ":sveltekit"];
    build.add_action(
        "ts:mathjax",
        EsbuildScript {
            script: "ts/transform_ts.mjs".into(),
            entrypoint: "ts/mathjax/index.ts".into(),
            deps: files.clone(),
            output_stem: "ts/mathjax/mathjax",
            extra_exts: &[],
        },
    )
}

pub const MATHJAX_FILES: &[&str] = &[
    "mathjax/es5/a11y/assistive-mml.js",
    "mathjax/es5/a11y/complexity.js",
    "mathjax/es5/a11y/explorer.js",
    "mathjax/es5/a11y/semantic-enrich.js",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_AMS-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Calligraphic-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Calligraphic-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Fraktur-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Fraktur-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-BoldItalic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Script-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size1-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size2-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size3-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size4-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Typewriter-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Vector-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Vector-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Zero.woff",
    "mathjax/es5/tex-chtml-full.js",
    "mathjax/es5/sre/mathmaps/de.json",
    "mathjax/es5/sre/mathmaps/en.json",
    "mathjax/es5/sre/mathmaps/es.json",
    "mathjax/es5/sre/mathmaps/fr.json",
    "mathjax/es5/sre/mathmaps/hi.json",
    "mathjax/es5/sre/mathmaps/it.json",
    "mathjax/es5/sre/mathmaps/nemeth.json",
];

pub fn copy_mathjax() -> impl BuildAction {
    RsyncFiles {
        inputs: inputs![":node_modules:mathjax"],
        target_folder: "qt/_aqt/data/web/js/vendor/mathjax",
        strip_prefix: "$builddir/node_modules/mathjax/es5",
        extra_args: "",
    }
}

fn build_sass(build: &mut Build) -> Result<()> {
    build.add_dependency("sass", inputs![glob!("ts/lib/sass/**")]);

    build.add_action(
        "css:_root-vars",
        CompileSass {
            input: inputs!["ts/lib/sass/_root-vars.scss"],
            output: "ts/lib/sass/_root-vars.css",
            deps: inputs![glob!["ts/lib/sass/*"]],
            load_paths: vec![],
        },
    )?;

    Ok(())
}

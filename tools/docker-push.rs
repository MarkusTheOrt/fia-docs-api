use std::process::Command;

pub fn main() {
    println!(
        "Pushing: \n- codeberg.org/mto/fia-docs-scraper:latest\n- codeberg.org/mto/fia-docs-scraper:{}",
        env!("CARGO_PKG_VERSION")
    );

    let status = Command::new("docker")
        .args(["push", "codeberg.org/mto/fia-docs-scraper:latest"])
        .status()
        .expect("Failed to run Docker build");

    if !status.success() {
        eprintln!("Docker push failed");
        std::process::exit(1);
    }

    let status = Command::new("docker")
        .args([
            "image",
            "tag",
            "codeberg.org/mto/fia-docs-scraper:latest",
            &format!(
                "codeberg.org/mto/fia-docs-scraper:{}",
                env!("CARGO_PKG_VERSION")
            ),
        ])
        .status()
        .expect("Failed to retag docker image");

    if !status.success() {
        eprintln!("Docker tag rename failed");
        std::process::exit(1);
    }

    let status = Command::new("docker")
        .args([
            "push",
            &format!(
                "codeberg.org/mto/fia-docs-scraper:{}",
                env!("CARGO_PKG_VERSION")
            ),
        ])
        .status()
        .expect("Failed to run docker push");

    if !status.success() {
        eprintln!("Docker push failed");
        std::process::exit(1);
    }
}

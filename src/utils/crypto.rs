use anyhow::Result;
use iroh::SecretKey;
use owo_colors::OwoColorize;
use rand::rngs::OsRng;

use crate::{cli::Opts, utils::constants::PRIVATE_KEY_PATH};

pub async fn load_secret_key(opts: &Opts) -> Result<SecretKey> {
    let path = opts.private_key.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .expect("Could not find home directory")
            .join(".punch")
            .join(PRIVATE_KEY_PATH)
    });

    if opts.regenerate {
        if opts.ephemeral {
            return Err(anyhow::anyhow!(
                "Cannot use {} with {}",
                "--regenerate".bold(),
                "--ephemeral".bold()
            ));
        }

        inquire::Confirm::new(&format!(
            "Regenerate secret key at {} ? This will overwrite the existing key.",
            path.display().purple()
        ))
        .with_default(false)
        .prompt()?;
        if !path.exists() {
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        }
        let sk = SecretKey::generate(&mut OsRng);
        write_secret_key(&path, &sk).await?;
        crate::success!("Regenerated secret key at {}", path.display().purple());
        return Ok(sk);
    }

    if path.exists() && !opts.ephemeral {
        let contents = tokio::fs::read(&path).await?;
        let bytes: [u8; 32] = contents
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }

    let sk = SecretKey::generate(&mut OsRng);
    if !opts.ephemeral {
        crate::info!("Generating new secret key at {}", path.display());
        write_secret_key(&path, &sk).await?;
    }
    Ok(sk)
}

async fn write_secret_key(path: &std::path::Path, sk: &SecretKey) -> Result<()> {
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(path, sk.to_bytes()).await?;
    Ok(())
}

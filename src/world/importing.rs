use crate::utils::prelude::*;
use fastanvil::{ChunkData, Region};
use indicatif::{ProgressBar, ProgressStyle};
use nbt_lib::NBTDeserializeBytes;
use rayon::prelude::*;
use std::env;
use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::state::GlobalState;
use crate::world::chunk_format::Chunk;

const DEFAULT_BATCH_SIZE: u8 = 150;

fn get_batch_size() -> i32 {
    let batch_size = env::args()
        .find(|x| x.starts_with("--batch_size="))
        .and_then(|x| {
            x.split('=')
                .last()
                .and_then(|s| s.parse::<i32>().ok())
        });

    match batch_size {
        Some(size) => {
            info!("Using custom batch size: {}", size);
            size
        }
        None => {
            info!("Using default batch size: {}", DEFAULT_BATCH_SIZE);
            info!("To change the batch size, use the --batch_size=<num> flag");
            DEFAULT_BATCH_SIZE as i32
        }
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if secs == 0 {
        format!("{}ms", millis)
    } else if secs < 60 {
        format!("{}s {}ms", secs, millis)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

async fn get_total_chunks(dir: &PathBuf) -> Result<usize> {
    let files = std::fs::read_dir(dir)?;
    let regions: Vec<Region<File>> = files
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().is_file() && entry.path().extension() == Some("mca".as_ref())
        })
        .filter_map(|entry| {
            match File::open(entry.path()) {
                Ok(file) => Region::from_stream(file).ok(),
                Err(_) => {
                    warn!("(Skipped) Could not read region file: {}", entry.path().display());
                    None
                }
            }
        })
        .collect();

    Ok(regions.into_par_iter().map(|mut region| region.iter().count()).sum())
}

fn process_chunk(chunk_data: Vec<u8>, file_name: &str, bar: Arc<ProgressBar>) -> Result<Chunk> {
    let mut final_chunk = Chunk::read_from_bytes(&mut Cursor::new(chunk_data))
        .map_err(|e| {
            bar.abandon_with_message(format!("Chunk {} failed to import", file_name));
            Error::Generic(format!("Could not read chunk {} {}", e, file_name))
        })?;

    final_chunk.convert_to_net_mode()
        .map_err(|e| {
            bar.abandon_with_message(format!("Chunk {} {} failed to import", final_chunk.x_pos, final_chunk.z_pos));
            Error::Generic(format!("Could not convert chunk {} {} to network mode: {}", final_chunk.x_pos, final_chunk.z_pos, e))
        })?;

    final_chunk.dimension = Some("overworld".to_string());
    Ok(final_chunk)
}

//noinspection RsBorrowChecker
pub async fn import_regions(state: GlobalState) -> Result<()> {
    let dir = get_import_directory()?;
    debug!("Starting import from: {}", dir.display());

    let start = std::time::Instant::now();
    info!("Analyzing world data... (this won't take long)");

    let total_chunks = get_total_chunks(&dir).await?;
    info!("Preparing to import {} chunks", total_chunks);
    info!("This process may take a while for large worlds. Please be patient.");

    let batch_size = get_batch_size() as usize;
    // let bar = create_progress_bar(total_chunks);
    let bar = Arc::new(create_progress_bar(total_chunks));

    let mut region_files = tokio::fs::read_dir(dir).await
        .map_err(|_| Error::Generic("Could not read the imports directory".to_string()))?;

    while let Some(dir_file) = region_files.next_entry().await? {
        let file_name = dir_file.file_name();
        let file_name = file_name.to_str().unwrap_or("unknown file");
        let file = File::open(dir_file.path())?;
        let mut region = Region::from_stream(file)?;

        let mut chunks: Vec<ChunkData> = region.iter().filter_map(|chunk| chunk.ok()).collect();

        // for chunk_batch in chunks.chunks(batch_size) {
        while !chunks.is_empty() {
            let chunk_batch: Vec<ChunkData> = chunks.drain(..std::cmp::min(batch_size, chunks.len())).collect();

            let start = std::time::Instant::now();
            let processed_chunks: Vec<Chunk> = chunk_batch.into_par_iter()
                .filter_map(|chunk| {
                    let data = chunk.data.clone();
                    match process_chunk(data, file_name, Arc::clone(&bar)) {
                        Ok(processed) => {
                            let bar = Arc::clone(&bar);
                            bar.inc(1);
                            Some(processed)
                        }
                        Err(e) => {
                            warn!("Failed to process chunk: {}. Skipping.", e);
                            None
                        }
                    }
                })
                .collect();
            info!("Processed {} chunks in {:?}", processed_chunks.len(), start.elapsed());

            // Insert the batch of processed chunks
            let start = std::time::Instant::now();
            let chunks_len = processed_chunks.len();
            insert_chunks(&state, processed_chunks, &bar).await?;
            info!("Inserted {} chunks in {:?}", chunks_len, start.elapsed());
        }

        /*
                let mut queued_chunks = Vec::new();

                for chunk in region.iter() {
                    let Ok(chunk) = chunk else {
                        warn!("Could not read chunk {file_name}. Skipping!");
                        continue;
                    };

                    let final_chunk = process_chunk(chunk, file_name, &bar).await?;

                    trace!("Queuing chunk {}, {}", final_chunk.x_pos, final_chunk.z_pos);
                    update_progress_bar(&bar, &final_chunk);

                    queued_chunks.push(final_chunk);

                    if queued_chunks.len() == batch_size as usize {
                        insert_chunks(&state, &mut queued_chunks, &bar).await?;
                    }
                }

                if !queued_chunks.is_empty() {
                    insert_chunks(&state, &mut queued_chunks, &bar).await?;
                }*/
    }

    finalize_import(&bar, total_chunks, start.elapsed());
    Ok(())
}

fn get_import_directory() -> Result<PathBuf> {
    if let Ok(root) = env::var("FERRUMC_ROOT") {
        Ok(PathBuf::from(root).join("import"))
    } else {
        env::current_exe()?
            .parent()
            .ok_or_else(|| Error::Generic("Failed to get exe directory".to_string()))
            .map(|path| path.join("import"))
    }
}

fn create_progress_bar(total_chunks: usize) -> ProgressBar {
    let bar = ProgressBar::new(total_chunks as u64);
    bar.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}").expect("Could not set progress bar style")
        .progress_chars("##-"));
    bar.set_message("Importing chunks...");
    bar
}

async fn insert_chunks(state: &GlobalState, queued_chunks: Vec<Chunk>, bar: &ProgressBar) -> Result<()> {
    state.database.batch_insert(queued_chunks).await
        .map_err(|e| {
            bar.abandon_with_message("Chunk insertion failed".to_string());
            Error::Generic(format!("Could not insert chunks: {}", e))
        })?;
    Ok(())
}

fn finalize_import(bar: &ProgressBar, total_chunks: usize, elapsed: std::time::Duration) {
    bar.finish_with_message(format!("Import complete! {} chunks processed.", total_chunks));
    info!(
        "Successfully imported {} chunks in {}",
        total_chunks,
        format_duration(elapsed)
    );
}

#[cfg(test)]
mod test {
    use crate::create_state;
    use crate::utils::prelude::*;
    use crate::utils::setup_logger;
    use tokio::net::TcpListener;

    #[tokio::test]
    #[ignore]
    async fn get_chunk_at() -> Result<()> {
        // set environment variable "FERRUMC_ROOT" to the root of the ferrumc project
        setup_logger()?;
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let state = create_state(listener).await.unwrap();

        let chunk = state
            .database
            .get_chunk(0, 0, "overworld".to_string())
            .await?
            .unwrap();

        println!("{:#?}", chunk);

        Ok(())
    }
}



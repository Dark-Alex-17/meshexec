use crate::config::Config;
use anyhow::{Result, anyhow};
use log::{error, info};
use meshtastic::api::ConnectedStreamApi;
use meshtastic::api::state::Configured;
use meshtastic::packet::{PacketDestination, PacketReceiver, PacketRouter};
use meshtastic::protobufs::from_radio;
use meshtastic::types::MeshChannel;
use std::error::Error;
use std::fmt::Display;
use std::mem;
use std::time::Duration;
use tokio::time::{sleep, timeout};

pub async fn wait_for_my_node_num(rx: &mut PacketReceiver) -> Result<u32> {
    let msg = timeout(Duration::from_secs(10), async {
        loop {
            let fr = rx.recv().await?;
            if let Some(from_radio::PayloadVariant::MyInfo(my_info)) = fr.payload_variant {
                return Some(my_info);
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for MyInfo. Is node online and connected?"))?
    .ok_or_else(|| anyhow!("rx closed before receiving MyInfo"))?;

    let node_num = msg.my_node_num;
    Ok(node_num)
}

pub fn chunk_lines_with_footer(text: &str, max_bytes: usize) -> Vec<String> {
    assert!(max_bytes > 0);

    let mut raw_chunks = Vec::new();
    let mut current = String::new();
    let mut current_bytes = 0usize;

    for line in text.split_inclusive('\n') {
        let line_bytes = line.len();

        if line_bytes > max_bytes {
            if !current.is_empty() {
                raw_chunks.push(mem::take(&mut current));
                current_bytes = 0;
            }

            let mut end = max_bytes.min(line.len());
            while end > 0 && !line.is_char_boundary(end) {
                end -= 1;
            }

            raw_chunks.push(line[..end].to_string());
            continue;
        }

        if current_bytes + line_bytes > max_bytes {
            raw_chunks.push(mem::take(&mut current));
            current_bytes = 0;
        }

        current.push_str(line);
        current_bytes += line_bytes;
    }

    if !current.is_empty() {
        raw_chunks.push(current);
    }

    let total = raw_chunks.len();

    raw_chunks
        .into_iter()
        .enumerate()
        .map(|(i, mut chunk)| {
            if total > 1 {
                let footer = format!("\n\n[{}/{}]", i + 1, total);
                let footer_bytes = footer.len();

                let available = max_bytes.saturating_sub(footer_bytes);
                if chunk.len() > available {
                    let mut end = available.min(chunk.len());
                    while end > 0 && !chunk.is_char_boundary(end) {
                        end -= 1;
                    }
                    chunk.truncate(end);
                }

                chunk.push_str(&footer);
            }
            chunk
        })
        .collect()
}

pub async fn send_split_text<R, E>(
    api: &mut ConnectedStreamApi<Configured>,
    router: &mut R,
    text: &str,
    server_config: &Config,
) -> Result<()>
where
    E: Display + Error + Send + Sync + 'static,
    R: PacketRouter<(), E>,
{
    let chunks = chunk_lines_with_footer(text, server_config.max_content_bytes);

    for (idx, part) in chunks.iter().enumerate() {
        info!("Sending chunk: {part}");
        let bytes = part.len();
        if bytes > server_config.max_text_bytes {
            error!(
                "part {} is {bytes} bytes (> {})",
                idx + 1,
                server_config.max_text_bytes
            );
            continue;
        }

        match api
            .send_text(
                router,
                part.clone(),
                PacketDestination::Broadcast,
                false,
                MeshChannel::from(server_config.channel),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("send_text failed on part {}: {e}", idx + 1);
                sleep(Duration::from_millis(server_config.chunk_delay)).await;
                api.send_text(
                    router,
                    part.clone(),
                    PacketDestination::Broadcast,
                    false,
                    MeshChannel::from(server_config.channel),
                )
                .await?;
            }
        }

        sleep(Duration::from_millis(server_config.chunk_delay)).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn chunk_empty_string_returns_empty_vec() {
        let chunks = chunk_lines_with_footer("", 10);
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_single_short_line_within_budget_no_footer() {
        let text = "hello";
        let chunks = chunk_lines_with_footer(text, 10);
        assert_eq!(chunks, vec![text.to_string()]);
    }

    #[test]
    fn chunk_two_lines_fit_in_one_chunk_no_footer() {
        let text = "alpha\nbeta\n";
        let chunks = chunk_lines_with_footer(text, 100);
        assert_eq!(chunks, vec![text.to_string()]);
    }

    #[test]
    fn chunk_two_lines_split_with_footers() {
        let text = "1234567\nabcdefg\n";
        let chunks = chunk_lines_with_footer(text, 15);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("1234567\n"));
        assert!(chunks[0].ends_with("[1/2]"));
        assert!(chunks[1].starts_with("abcdefg\n"));
        assert!(chunks[1].ends_with("[2/2]"));
    }

    #[test]
    fn chunk_single_long_line_truncates_to_max_bytes() {
        let text = "abcdefghij";
        let chunks = chunk_lines_with_footer(text, 5);
        assert_eq!(chunks, vec!["abcde".to_string()]);
    }

    #[test]
    fn chunk_footer_accounting_keeps_chunks_within_max_bytes() {
        let text = "1234567\nabcdefg\n";
        let max_bytes = 15;
        let chunks = chunk_lines_with_footer(text, max_bytes);
        for chunk in &chunks {
            assert!(chunk.len() <= max_bytes);
        }
    }

    #[test]
    fn chunk_respects_utf8_char_boundaries() {
        let text = "héllo";
        let chunks = chunk_lines_with_footer(text, 2);
        assert_eq!(chunks, vec!["h".to_string()]);
    }

    #[test]
    fn chunk_handles_trailing_newline() {
        let text = "hello\n";
        let chunks = chunk_lines_with_footer(text, 100);
        assert_eq!(chunks, vec![text.to_string()]);
    }

    #[test]
    fn chunk_three_chunks_have_correct_footers() {
        let text = "1234567\nabcdefg\nqwertyu\n";
        let chunks = chunk_lines_with_footer(text, 15);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].ends_with("[1/3]"));
        assert!(chunks[1].ends_with("[2/3]"));
        assert!(chunks[2].ends_with("[3/3]"));
    }

    #[test]
    fn chunk_max_bytes_one_still_works() {
        let text = "ab";
        let chunks = chunk_lines_with_footer(text, 1);
        assert_eq!(chunks, vec!["a".to_string()]);
    }

    proptest! {
        #[test]
        fn chunk_output_never_exceeds_max_bytes(
            text in "[ -~\n]{0,500}",
            max_bytes in 10usize..256
        ) {
            let chunks = chunk_lines_with_footer(&text, max_bytes);
            for chunk in &chunks {
                prop_assert!(
                    chunk.len() <= max_bytes,
                    "chunk len {} exceeded max_bytes {}: {:?}",
                    chunk.len(), max_bytes, chunk
                );
            }
        }

        #[test]
        fn chunk_preserves_all_content_when_single_chunk(
            text in "[a-z]{1,50}"
        ) {
            let chunks = chunk_lines_with_footer(&text, 1000);
            prop_assert_eq!(chunks.len(), 1);
            prop_assert_eq!(&chunks[0], &text);
        }

        #[test]
        fn chunk_count_footer_format(
            text in "[a-z ]{20,200}\n[a-z ]{20,200}\n",
            max_bytes in 20usize..60
        ) {
            let chunks = chunk_lines_with_footer(&text, max_bytes);
            let total = chunks.len();
            if total > 1 {
                for (i, chunk) in chunks.iter().enumerate() {
                    let expected_footer = format!("[{}/{}]", i + 1, total);
                    prop_assert!(
                        chunk.ends_with(&expected_footer),
                        "chunk {} missing footer: {:?}",
                        i, chunk
                    );
                }
            }
        }
    }
}

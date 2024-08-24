use happychain::{Error, Message, Sender};
use web3::{transports::Http, types::U256};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let transport = Http::new("http://localhost:8545")?;
    let web3 = web3::Web3::new(transport);
    let accounts = web3.eth().accounts().await?;
    let sender = Sender::new(web3.transport().clone()).await?;

    let msg1 = Message::new(
        accounts[0],
        Some(accounts[1]),
        Some(U256::from(21_000u64)),
        Some(U256::from(1_000_000_000_000_000_000u64)), // 1 ETH
        None,
        1,
        vec![],
        None,
    )?;

    sender.add_message(msg1).await?;

    sender.run().await
}

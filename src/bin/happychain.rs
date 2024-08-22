use happychain::{Result, Sender, Transaction};
use web3::transports::Http;
use happychain::gas_price::TransactionUrgency;

#[tokio::main]
async fn main() -> Result<()> {
    let transport = Http::new("http://localhost:8545")?;
    let web3 = web3::Web3::new(transport);
    let accounts = web3.eth().accounts().await?;
    let sender = Sender::new(web3.transport().clone(), accounts[0]).await?;

    let tx1 = Transaction::new(
        web3::types::TransactionParameters::default(),
        1,
        TransactionUrgency::Medium,
        vec![],
    )?;

    sender.add_transaction(tx1).await?;

    sender.run().await
}

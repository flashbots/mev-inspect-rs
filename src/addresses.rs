use super::types::Protocol;

use ethers::types::Address;

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};

pub fn lookup(address: Address) -> String {
    ADDRESSBOOK
        .get(&address)
        .unwrap_or(&format!("{:?}", &address).to_string())
        .clone()
}

fn insert_many<T: Clone>(
    mut map: HashMap<Address, T>,
    addrs: &[&str],
    value: T,
) -> HashMap<Address, T> {
    for addr in addrs {
        map.insert(parse_address(addr), value.clone());
    }
    map
}

// Uniswap-like addresses
pub static UNISWAP: Lazy<HashMap<Address, Protocol>> = Lazy::new(|| {
    let map = HashMap::new();
    let map = insert_many(
        map,
        &[
            "0x2b095969ae40BcE8BaAF515B16614A636C22a6Db",
            "0x2fdbadf3c4d5a8666bc06645b8358ab803996e28",
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d",
        ],
        Protocol::Uniswap,
    );

    let map = insert_many(
        map,
        &[
            // Sushi YFI
            "0x088ee5007c98a9677165d78dd2109ae4a3d04d0c",
            // Sushi router
            "d9e1cE17f2641f24aE83637ab66a2cca9C378B9F",
        ],
        Protocol::Sushiswap,
    );

    map
});

pub static FILTER: Lazy<HashSet<Address>> = Lazy::new(|| {
    let mut set = HashSet::new();
    // 1inch
    set.insert(parse_address("0x11111254369792b2ca5d084ab5eea397ca8fa48b"));
    set
});

pub static AAVE_LENDING_POOL: Lazy<Address> =
    Lazy::new(|| parse_address("398eC7346DcD622eDc5ae82352F02bE94C62d119"));

pub static WETH: Lazy<Address> =
    Lazy::new(|| parse_address("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));

pub static ETH: Lazy<Address> =
    Lazy::new(|| parse_address("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"));

pub static ADDRESSBOOK: Lazy<HashMap<Address, String>> = Lazy::new(|| {
    // TODO: Read these from a CSV?
    let map: HashMap<Address, String> = [
        // Contracts
        (
            "0x2fdbadf3c4d5a8666bc06645b8358ab803996e28",
            "UniswapPair YFI 8",
        ),
        (
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d",
            "Uniswap Router V2",
        ),
        (
            "0x088ee5007C98a9677165D78dD2109AE4a3D04d0C",
            "Sushiswap: YFI",
        ),
        (
            "0x7c66550c9c730b6fdd4c03bc2e73c5462c5f7acc",
            "Kyber: Contract 2",
        ),
        (
            "0x10908c875d865c66f271f5d3949848971c9595c9",
            "Kyber: Reserve Uniswap V2",
        ),
        (
            "0x3dfd23a6c5e8bbcfc9581d2e864a68feb6a076d3",
            "AAVE: Lending Pool Core",
        ),
        (
            "0xb6ad5fd2698a68917e39216304d4845625da2f57",
            "Balancer: YFI/yyDAI+yUSDC+yUSDT+yTUSD 50/50",
        ),
        (
            "0xd44082f25f8002c5d03165c5d74b520fbc6d342d",
            "Balancer: Pool 293 (YFI / LEND / MKR / WETH / LINK)",
        ),
        // Tokens
        ("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", "USDC"),
        ("0x0000000000000000000000000000000000000000", "ETH"),
        ("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "ETH"),
        ("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "WETH"),
        ("0x0bc529c00c6401aef6d220be8c6ea1667f6ad93e", "YFI"),
        (
            "0x5dbcf33d8c2e976c6b560249878e6f1491bca25c",
            "yyDAI+yUSDC+yUSDT+yTUSD",
        ),
    ]
    .iter()
    .map(|(addr, token)| (parse_address(addr), token.to_string()))
    .collect();

    // https://github.com/flashbots/mev-inspect/blob/master/src/InspectorKnownBot.ts#L17
    let map = insert_many(
        map,
        &[
            "0x9799b475dec92bd99bbdd943013325c36157f383",
            "0xad572bba83cd36902b508e89488b0a038986a9f3",
            "0x00000000553a85582988aa8ad43fb7dda2466bc7",
            "0xa619651c323923ecd5a8e5311771d57ac7e64d87",
            "0x0000000071e801062eb0544403f66176bba42dc0",
            "0x5f3e759d09e1059e4c46d6984f07cbb36a73bdf1",
            "0x000000000000084e91743124a982076c59f10084",
            "0x00000000002bde777710c370e08fc83d61b2b8e1",
            "0x42d0ba0223700dea8bca7983cc4bf0e000dee772",
            "0xfd52a4bd2289aeccf8521f535ec194b7e21cdc96",
            "0xfe7f0897239ce9cc6645d9323e6fe428591b821c",
            "0x7ee8ab2a8d890c000acc87bf6e22e2ad383e23ce",
            "0x860bd2dba9cd475a61e6d1b45e16c365f6d78f66",
            "0x78a55b9b3bbeffb36a43d9905f654d2769dc55e8",
            "0x2204b8bd8c62c632df16af1475554d07e75769f0",
            "0xe33c8e3a0d14a81f0dd7e174830089e82f65fc85",
            "0xb958a8f59ac6145851729f73c7a6968311d8b633",
            "0x3144d9885e57e6931cf51a2cac6a70dad6b805b2",
            "0x000000000000006f6502b7f2bbac8c30a3f67e9a",
            "0x42a65ebdcce01d41a6e9f94b7367120fa78d26fe",
            "0x6780846518290724038e86c98a1e903888338875",
            "0xa21a415b78767166ee222c92bf4b47b6c2f916e0",
            "0xf9bf440b8b8423b472c646c3e51aa5e3d04a66f4",
            "0xd1c300000000b961df238700ef00600097000049",
            "0xd39169726d64d18add3dbbcb3cef12f36db0c70a",
            "0x00000000000017c75025d397b91d284bbe8fc7f2",
            "0x000000000025d4386f7fb58984cbe110aee3a4c4",
            "0x72b94a9e3473fdd9ecf3da7dd6cc6bb218ae79e3",
            "0x6cdc900324c935a2807ecc308f8ead1fcd62fe35",
            "0x435c90cdbbe09fa5a862a291b79c1623adbe16d0",
            "0xb00ba6778cf84100da676101e011b3d229458270",
            "0xb00ba6e641a3129b8c515bb14a4c1bba32d2e8df",
            "0x8a3960472b3d63894b68df3f10f58f11828d6fd9",
            "0xb8db34f834e9df42f2002ceb7b829dad89d08e14",
            "0x7e2deaa00273d0b4ef1ceef712e7d9f812df3e8a",
            "0x3d71d79c224998e608d03c5ec9b405e7a38505f0",
            "0xff73257d2bee2cce718010205cb2c1bb7755db24",
            "0x245b47669f44fc23b6e841953b7cc0a7bbdba9ef",
            "0x0000000000007f150bd6f54c40a34d7c3d5e9f56",
            "0x7c651d7084b4ba899391d2d4d5d3d47fff823351",
            "0x661c650c8bfcde6d842f465b3d69ed008638d614",
            "0x175789024955c56b06a618806fc13df71d08a377",
            // "0x00000000000080c886232e9b7ebbfb942b5987aa",
            "0x8be4db5926232bc5b02b841dbede8161924495c4", // sandwich bot
            // Old ones, for back-fill
            "0x0000000000009480cded7b47d438e73edf0f67e5",
            "0x18d81d985d585405688ef7c62806152cf797ae37",
            "0x000000000000a32dc5dd625c107898a1c72ad34a",
            "0x1b1e08043553cad2a3b82bfc2df40f7dcc0d58aa",
            "0x18f60c7bd9fb6619b807d8d81334f1760c69fb59",
            "0xb87c7d5a5ff0092cf427855c1ea9b7708d717292",
            // Aave Kyber Uni liquidation
            "0x80119949f52cb9bf18ecf259e3c3b59f0e5e5a5b",
        ],
        "KNOWN BOT".to_string(),
    );

    map
});

// Map of protocols
fn parse_address(addr: &str) -> Address {
    if addr.starts_with("0x") {
        addr[2..].parse().unwrap()
    } else {
        addr.parse().unwrap()
    }
}

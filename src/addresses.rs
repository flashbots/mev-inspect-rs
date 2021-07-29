use super::types::Protocol;

use ethers::types::Address;

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

pub fn lookup(address: Address) -> String {
    ADDRESSBOOK
        .get(&address)
        .unwrap_or(&format!("{:?}", &address))
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

// reads line-separated addresses from a file path and returns them as a vector
fn read_addrs<P>(path: P) -> Vec<Address>
where
    P: AsRef<Path>,
{
    let file = File::open(path).unwrap();
    let lines = io::BufReader::new(file).lines();
    lines.map(|line| parse_address(&line.unwrap())).collect()
}

// Protocol Addrs
pub static PROTOCOLS: Lazy<HashMap<Address, Protocol>> = Lazy::new(|| {
    let map = HashMap::new();
    let map = insert_many(
        map,
        &[
            "0x9346c20186d1794101b8517177a1b15c49c9ff9b",
            "0x2b095969ae40BcE8BaAF515B16614A636C22a6Db",
            "0x2fdbadf3c4d5a8666bc06645b8358ab803996e28",
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d",
            "0xb4e16d0168e52d35cacd2c6185b44281ec28c9dc",
            "0xDcD6011f4C6B80e470D9487f5871a0Cba7C93f48", // 0x: UniswapV2Bridge
        ],
        Protocol::UniswapV2,
    );

    let mut map = insert_many(
        map,
        &[
            // sUSD - WETH
            "0xf1f85b2c54a2bd284b1cf4141d64fd171bd85539",
            // Sushi YFI
            "0x088ee5007c98a9677165d78dd2109ae4a3d04d0c",
            // Sushi router
            "d9e1cE17f2641f24aE83637ab66a2cca9C378B9F",
        ],
        Protocol::Sushiswap,
    );

    for addr in read_addrs("./res/v1pairs.csv") {
        map.insert(addr, Protocol::UniswapV1);
    }

    for addr in read_addrs("./res/v2pairs.csv") {
        map.insert(addr, Protocol::UniswapV2);
    }

    for addr in read_addrs("./res/sushipairs.csv") {
        map.insert(addr, Protocol::Sushiswap);
    }

    // uni router 02
    map.insert(
        parse_address("7a250d5630B4cF539739dF2C5dAcb4c659F2488D"),
        Protocol::UniswapV2,
    );

    // uni router 01
    map.insert(
        parse_address("f164fC0Ec4E93095b804a4795bBe1e041497b92a"),
        Protocol::UniswapV2,
    );

    // sushi router
    map.insert(
        "d9e1cE17f2641f24aE83637ab66a2cca9C378B9F".parse().unwrap(),
        Protocol::Sushiswap,
    );

    // 0x
    map.insert(*ZEROX, Protocol::ZeroEx);

    // dydx
    map.insert(*DYDX, Protocol::DyDx);

    // balancer
    map.insert(*BALANCER_PROXY, Protocol::Balancer);

    insert_many(
        map,
        &["0xfe01821Ca163844203220cd08E4f2B2FB43aE4E4"], // 0x: BalancerBridge
        Protocol::Balancer,
    )
});

// Addresses which should be ignored when used as the target of a transaction
pub static FILTER: Lazy<HashSet<Address>> = Lazy::new(|| {
    let mut set = HashSet::new();
    // 1inch
    set.insert(parse_address("0x11111254369792b2ca5d084ab5eea397ca8fa48b"));
    // 1inch v2
    set.insert(parse_address("0x111111125434b319222cdbf8c261674adb56f3ae"));
    // 1inch v3 router
    set.insert(parse_address("0x11111112542d85b3ef69ae05771c2dccff4faa26"));
    // paraswap
    set.insert(parse_address("0x9509665d015bfe3c77aa5ad6ca20c8afa1d98989"));
    // paraswap v2
    set.insert(parse_address("0x86969d29F5fd327E1009bA66072BE22DB6017cC6"));
    // Paraswap v3
    set.insert(parse_address("0xf90e98f3d8dce44632e5020abf2e122e0f99dfab"));
    // furucombo
    set.insert(parse_address("0x57805e5a227937bac2b0fdacaa30413ddac6b8e1"));
    // furucombo proxy v1
    set.insert(parse_address("0x17e8ca1b4798b97602895f63206afcd1fc90ca5f"));
    // yearn recycler
    set.insert(parse_address("0x5F07257145fDd889c6E318F99828E68A449A5c7A"));
    // drc, weird deflationary token
    set.insert(parse_address("0xc66d62a2f9ff853d9721ec94fa17d469b40dde8d"));
    // Rootkit finance deployer
    set.insert(parse_address("0x804cc8d469483d202c69752ce0304f71ae14abdf"));
    // Metamask Swap
    set.insert(parse_address("0x881d40237659c251811cec9c364ef91dc08d300c"));
    // DEX.ag
    set.insert(parse_address("0x745daa146934b27e3f0b6bff1a6e36b9b90fb131"));
    // Cream Finance deployer
    set.insert(parse_address("0x197939c1ca20c2b506d6811d8b6cdb3394471074"));
    // Zerion SDK
    set.insert(parse_address("0xb2be281e8b11b47fec825973fc8bb95332022a54"));
    // KeeperDAO
    set.insert(parse_address("0x3d71d79c224998e608d03c5ec9b405e7a38505f0"));
    // ParaSwap P4
    set.insert(parse_address("0x1bd435f3c054b6e901b7b108a0ab7617c808677b"));
    set
});

pub static ZEROX: Lazy<Address> =
    Lazy::new(|| parse_address("0x61935cbdd02287b511119ddb11aeb42f1593b7ef"));

pub static DYDX: Lazy<Address> =
    Lazy::new(|| parse_address("0x1e0447b19bb6ecfdae1e4ae1694b0c3659614e4e"));

pub static BALANCER_PROXY: Lazy<Address> =
    Lazy::new(|| parse_address("0x3E66B66Fd1d0b02fDa6C811Da9E0547970DB2f21"));

pub static CURVE_REGISTRY: Lazy<Address> =
    Lazy::new(|| parse_address("0x7D86446dDb609eD0F5f8684AcF30380a356b2B4c"));

pub static CETH: Lazy<Address> =
    Lazy::new(|| parse_address("4Ddc2D193948926D02f9B1fE9e1daa0718270ED5"));

pub static COMPTROLLER: Lazy<Address> =
    Lazy::new(|| parse_address("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B"));

pub static COMP_ORACLE: Lazy<Address> =
    Lazy::new(|| parse_address("922018674c12a7F0D394ebEEf9B58F186CdE13c1"));

pub static AAVE_LENDING_POOL: Lazy<Address> =
    Lazy::new(|| parse_address("398eC7346DcD622eDc5ae82352F02bE94C62d119"));

pub static AAVE_LENDING_POOL_CORE: Lazy<Address> =
    Lazy::new(|| parse_address("3dfd23a6c5e8bbcfc9581d2e864a68feb6a076d3"));

pub static WETH: Lazy<Address> =
    Lazy::new(|| parse_address("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));

pub static ETH: Lazy<Address> =
    Lazy::new(|| parse_address("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"));

pub static ADDRESSBOOK: Lazy<HashMap<Address, String>> = Lazy::new(|| {
    // TODO: Read these from a CSV?
    let map: HashMap<Address, String> = [
        // 0x Exchange Proxies
        (
            "0xdef1c0ded9bec7f1a1670819833240f027b25eff",
            "0x: ExchangeProxy",
        ),
        (
            "0xfe01821Ca163844203220cd08E4f2B2FB43aE4E4",
            "0x: BalancerBridge",
        ),
        (
            "0xDcD6011f4C6B80e470D9487f5871a0Cba7C93f48",
            "0x: UniswapV2Bridge",
        ),
        (
            "0x761C446DFC9f7826374ABDeCc79F992e7F17330b",
            "0x: TranformERC20",
        ),
        // Contracts
        (
            "0x2fdbadf3c4d5a8666bc06645b8358ab803996e28",
            "UniswapPair YFI 8",
        ),
        (
            "0x3dA1313aE46132A397D90d95B1424A9A7e3e0fCE",
            "UniswapPair CRV 8",
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
        ("0xe41d2489571d322189246dafa5ebde1f4699f498", "ZRX"),
        ("0x0d8775f648430679a709e98d2b0cb6250d2887ef", "BAT"),
        ("0xd533a949740bb3306d119cc777fa900ba034cd52", "CRV"),
        ("0x80fb784b7ed66730e8b1dbd9820afd29931aab03", "LEND"),
        ("0x6B175474E89094C44DA98B954EEDEAC495271D0F", "DAI"),
        ("0xc00e94cb662c3520282e6f5717214004a7f26888", "COMP"),
        ("0x5d3a536e4d6dbd6114cc1ead35777bab948e3643", "cDAI"),
        ("0x514910771af9ca656af840dff83e8264ecf986ca", "LINK"),
        ("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", "WBTC"),
        ("0xdac17f958d2ee523a2206206994597c13d831ec7", "USDT"),
        ("0x57ab1ec28d129707052df4df418d58a2d46d5f51", "sUSD"),
        (
            "0x5dbcf33d8c2e976c6b560249878e6f1491bca25c",
            "yyDAI+yUSDC+yUSDT+yTUSD",
        ),
        ("0x0000000000b3f879cb30fe243b4dfee438691c04", "GST2"),
    ]
    .iter()
    .map(|(addr, token)| (parse_address(addr), token.to_string()))
    .collect();

    // https://github.com/flashbots/mev-inspect/blob/master/src/InspectorKnownBot.ts#L17
    insert_many(
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
    )
});

pub fn parse_address(addr: &str) -> Address {
    let addr = addr.strip_prefix("0x").unwrap_or(addr);
    addr.parse().unwrap()
}

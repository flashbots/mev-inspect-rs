var Web3 = require('web3');
var web3 = new Web3('ws://localhost:8546');

var fs = require('fs');

var dydx_abi;
var dydx;
var router_abi;
var router;
let daiDec = web3.utils.toBN(10).pow(web3.utils.toBN(18));
let usdcDec = web3.utils.toBN(10).pow(web3.utils.toBN(6));

fs.readFile('./router.json', 'utf8', function(err, contents) {
  router_abi = JSON.parse(contents);
  fs.readFile('./dydx.json', 'utf8', function(err, contents) {
      dydx_abi = JSON.parse(contents);
      dydx = new web3.eth.Contract(dydx_abi, "0x1e0447b19bb6ecfdae1e4ae1694b0c3659614e4e");
      router = new web3.eth.Contract(router_abi, "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D");
      get_liqs(router);
  });
});

market_map = {
  '0': "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
  '1': "0x89d24a6b4ccb1b6faa2625fe562bdd9a23260359",
  '2': "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
  '3': "0x6b175474e89094c44da98b954eedeac495271d0f"
}

async function get_liqs(router) {
  console.log("getting all liquidations for dydx");
  let block = await web3.eth.getBlock("latest");
  let end = block.number;
  var j = 0;
  let size = 1000;
  let start = 9220000; //end - 100*size;
  let txs = {};
  while ( j < (end - start) / size) {
    let tx;
    console.log(start + j*size, start + j*size + size);
    var all_liqs = await dydx.getPastEvents("LogLiquidate", {fromBlock: start + j*size, toBlock: start + j*size + size});
    for (let i = 0; i < all_liqs.length; i++) {
      if (!txs[all_liqs[i].transactionHash]) {
        txs[all_liqs[i].transactionHash] = {
          profits: web3.utils.toBN(0)
        };
      }
      let liq = all_liqs[i]['returnValues'];
      let liquidation = {
        sent_token: market_map[liq['owedMarket']],
        sent_amount: web3.utils.toBN(liq['solidOwedUpdate']['deltaWei']['value']),
        received_token: market_map[liq['heldMarket']],
        received_amount: web3.utils.toBN(liq['solidHeldUpdate']['deltaWei']['value']),
        from: liq['solidAccountOwner'],
        liquidated_user: liq['liquidAccountOwner']
      }

      if (!txs[all_liqs[i].transactionHash]['actions']) {
        txs[all_liqs[i].transactionHash]["actions"] = [];
      }

      txs[all_liqs[i].transactionHash]["actions"].push("Liquidation");


      if (!txs[all_liqs[i].transactionHash]['gas_price']) {
        tx = await web3.eth.getTransactionReceipt(all_liqs[i].transactionHash);
        let sent_as = await web3.eth.getTransaction(all_liqs[i].transactionHash);
        txs[all_liqs[i].transactionHash]["gas_price"] = sent_as.gasPrice;
        txs[all_liqs[i].transactionHash]["gas_used"] = tx.gasUsed;
        txs[all_liqs[i].transactionHash]["status"] = tx.status;
        txs[all_liqs[i].transactionHash]["eoa"] = sent_as.from;
      }

      let prices = await quotes(router, liquidation.sent_token,  liquidation.sent_amount, liquidation.received_token, liquidation.received_amount, tx.blockNumber);
      txs[all_liqs[i].transactionHash]["profits"] = txs[all_liqs[i].transactionHash]["profits"].add(prices[1].sub(prices[0]));
    }
    j += 1;
  }

  var replacer = function(key, value) { return value === null ? '' : value }
  let csv = "hash,profits,actions,gas_price,gas,status,eoa\r\n"
  for (const [txhash, liq] of Object.entries(txs)) {
    csv += JSON.stringify(txhash, replacer) + ","
    for (const [field, inner_value] of Object.entries(liq)) {
      csv += JSON.stringify(inner_value.toString(), replacer) + ","
    }
     csv += '\r\n'
  }

  fs.writeFile("./dydx_liqs.csv", csv, function(err) {
    console.log("writing to dydx_liqs")
    if (err) console.log(err);
    return;
  });
}

async function quotes(router, sent, sent_amt, received, received_amt, blockNum) {
    let sentPrice;
    let receivedPriced;

    let fullSent;
    let fullReceieved;

    if (sent == market_map['2']) {
      let amounts = await router.methods.getAmountsOut(usdcDec, [sent, market_map['0']]).call({}, blockNum);
      sentPrice =  web3.utils.toBN(
        amounts[1]
      );
      fullSent = sentPrice.mul(sent_amt).div(usdcDec);

    } else if (sent == market_map['0']){
      fullSent = sent_amt;
    } else {
      let amounts = await router.methods.getAmountsOut(daiDec, [sent, market_map['0']]).call({}, blockNum);
      sentPrice =  web3.utils.toBN(
        amounts[1]
      );
      fullSent = sentPrice.mul(sent_amt).div(daiDec);
    }

    if (received == market_map['2']) {
      receivedPrice =  web3.utils.toBN(
        await router.methods.getAmountsOut(usdcDec, [received, market_map['0']]).call({}, blockNum)[1]
      );
      fullReceieved = receivedPrice.mul(receieved_amt).div(usdcDec);
    } else if (sent == market_map['0']){
      fullReceieved = received_amt;
    } else {
      receivedPrice = web3.utils.toBN(
          await router.methods.getAmountsOut(daiDec, [received, market_map['0']]).call({}, blockNum)[1]
      );
      fullReceieved = receivedPrice.mul(receieved_amt).div(daiDec);
    }

    return [fullSent, fullReceieved];
}

# Script to get all Uni V1, V2 and Sushiswap pair addresses.
# Requires web3.py installed (`pip install web3`)
# URL=http://localhost:8545 python addrs.py
from web3 import Web3, HTTPProvider
import os

# abis
v1abi = [{"name": "NewExchange", "inputs": [{"type": "address", "name": "token", "indexed": True}, {"type": "address", "name": "exchange", "indexed": True}], "anonymous": False, "type": "event"}]
v2abi = [{"inputs":[{"internalType":"address","name":"_feeToSetter","type":"address"}],"payable":False,"stateMutability":"nonpayable","type":"constructor"},{"anonymous":False,"inputs":[{"indexed":True,"internalType":"address","name":"token0","type":"address"},{"indexed":True,"internalType":"address","name":"token1","type":"address"},{"indexed":False,"internalType":"address","name":"pair","type":"address"},{"indexed":False,"internalType":"uint256","name":"","type":"uint256"}],"name":"PairCreated","type":"event"},{"constant":True,"inputs":[{"internalType":"uint256","name":"","type":"uint256"}],"name":"allPairs","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":False,"stateMutability":"view","type":"function"},{"constant":True,"inputs":[],"name":"allPairsLength","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"payable":False,"stateMutability":"view","type":"function"},{"constant":False,"inputs":[{"internalType":"address","name":"tokenA","type":"address"},{"internalType":"address","name":"tokenB","type":"address"}],"name":"createPair","outputs":[{"internalType":"address","name":"pair","type":"address"}],"payable":False,"stateMutability":"nonpayable","type":"function"},{"constant":True,"inputs":[],"name":"feeTo","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":False,"stateMutability":"view","type":"function"},{"constant":True,"inputs":[],"name":"feeToSetter","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":False,"stateMutability":"view","type":"function"},{"constant":True,"inputs":[{"internalType":"address","name":"","type":"address"},{"internalType":"address","name":"","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"","type":"address"}],"payable":False,"stateMutability":"view","type":"function"},{"constant":False,"inputs":[{"internalType":"address","name":"_feeTo","type":"address"}],"name":"setFeeTo","outputs":[],"payable":False,"stateMutability":"nonpayable","type":"function"},{"constant":False,"inputs":[{"internalType":"address","name":"_feeToSetter","type":"address"}],"name":"setFeeToSetter","outputs":[],"payable":False,"stateMutability":"nonpayable","type":"function"}]

# addrs
V1FACTORY = "0xc0a47dFe034B400B47bDaD5FecDa2621de6c4d95"
V2FACTORY = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f"
SUSHIFACTORY = "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac"

# deployed at blocks
V1DEPLOY = 0x65224d
V2DEPLOY = 0x9899c3
SUSHIDEPLOY = 0xa4b4f5

URL = os.environ["URL"]
w3 = Web3(HTTPProvider(URL))

def dump(fname, data):
    with open(fname, 'w') as f:
        for item in data:
            f.write("%s\n" % item)

# if you get RPC errors, run each section separately
contract = w3.eth.contract(address = V1FACTORY, abi=v1abi)
events = contract.events.NewExchange.createFilter(fromBlock=V1DEPLOY).get_all_entries()
pairs = [e.args.exchange for e in events]
dump("./res/v1pairs.csv", pairs)

contract = w3.eth.contract(address = V2FACTORY, abi=v2abi)
events = contract.events.PairCreated.createFilter(fromBlock=V2DEPLOY).get_all_entries()
pairs = [e.args.pair for e in events]
dump("./res/v2pairs.csv", pairs)
 
contract = w3.eth.contract(address = SUSHIFACTORY, abi=v2abi)
events = contract.events.PairCreated.createFilter(fromBlock=SUSHIDEPLOY).get_all_entries()
pairs = [e.args.pair for e in events]
dump("./res/sushipairs.csv", pairs)

// Allow `cargo stylus export-abi` to generate a main function.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

/// Use an efficient WASM allocator.
#[global_allocator]
static ALLOC: mini_alloc::MiniAlloc = mini_alloc::MiniAlloc::INIT;

/// Import items from the SDK. The prelude contains common traits and macros.
use stylus_sdk::{
    alloy_primitives::{Address, U256},
    alloy_sol_types::sol,
    block, msg,
    prelude::*,
};

sol_interface! {
    interface IERC20 {
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 value) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 value) external returns (bool);
        function transferFrom(address from, address to, uint256 value) external returns (bool);
    }

    interface IMigratorChef {
        // Perform LP token migration from legacy UniswapV2 to SushiSwap.
        // Take the current LP token address and return the new LP token address.
        // Migrator should have full access to the caller's LP token.
        // Return the new LP token address.
        //
        // XXX Migrator must have allowance access to UniswapV2 LP tokens.
        // SushiSwap must mint EXACTLY the same amount of SushiSwap LP tokens or
        // else something bad will happen. Traditional UniswapV2 does not
        // do that so be careful!
        function migrate(address token) external returns (address);
    }
}

sol_storage! {
    pub struct UserInfo {
        uint256 amount; // How many LP tokens the user has provided.
        uint256 reward_debt; // Reward debt.
    }

    pub struct PoolInfo {
        address lp_token; // Address of LP token contract.
        uint256 alloc_point; // How many allocation points assigned to this pool. SUSHIs to distribute per block.
        uint256 last_reward_block; // Last block number that SUSHIs distribution occurs.
        uint256 acc_sushi_per_share; // Accumulated SUSHIs per share, times 1e12. See below.

    }

    #[entrypoint]
    pub struct MasterChef {
        address owner; // Owner.
        address sushi; // The SUSHI TOKEN!
        address dev_addr; // Dev address.
        uint256 bonus_end_block; // Block number when bonus SUSHI period ends.
        uint256 sushi_per_block; // SUSHI tokens created per block.
        // bonus multplier = 10;
        address migrator; // The migrator contract.
        PoolInfo[] pool_info; // Info of each pool.
        mapping(uint256 => mapping(address => UserInfo)) user_info; // Info of each user that stakes LP tokens.
        uint256 total_alloc_point; // Must be the sum of all alloc points in all pools.
        uint256 start_block; // The block number when SUSHI mining starts.
    }
}

sol! {
    event Deposit(address indexed user, uint256 indexed pid, uint256 amount);
    event Withdraw(address indexed user, uint256 indexed pid, uint256 amount);
    event EmergencyWithdraw(address indexed user, uint256 indexed pid, uint256 amount);

    error AlreadyInitialized();
    error NonOwner();
    error NotDevAddress();
}

#[derive(SolidityError)]
pub enum MasterChefError {
    AlreadyInitialized(AlreadyInitialized),
    NonOwner(NonOwner),
    NotDevAddress(NotDevAddress),
}

#[external]
impl MasterChef {
    // Ownable functions...
    // pub const BONUS_MULTIPLIER: U256 = 10;

    pub fn pool_length(&self) -> U256 {
        U256::from(self.pool_info.len())
    }

    /// Initialize - Constructor.
    pub fn init(
        &mut self,
        sushi: Address,
        dev_addr: Address,
        bonus_end_block: U256,
        sushi_per_block: U256,
        start_block: U256,
    ) -> Result<(), MasterChefError> {
        if self.owner.get() != Address::default() {
            return Err(MasterChefError::AlreadyInitialized(AlreadyInitialized {}));
        }

        self.owner.set(msg::sender());
        self.sushi.set(sushi);
        self.dev_addr.set(dev_addr);
        self.bonus_end_block.set(bonus_end_block);
        self.sushi_per_block.set(sushi_per_block);
        self.start_block.set(start_block);

        Ok(())
    }

    /// Admin functions
    pub fn add(
        &mut self,
        alloc_point: U256,
        lp_token: Address,
        with_update: bool,
    ) -> Result<(), MasterChefError> {
        // onlyOwner modifier.
        if self.owner.get() != msg::sender() {
            return Err(MasterChefError::NonOwner(NonOwner {}));
        }

        if with_update {
            // massUpdatePool()
        }

        let last_reward_block: U256 = if U256::from(block::timestamp()) > self.start_block.get() {
            U256::from(block::number())
        } else {
            self.start_block.get()
        };

        self.total_alloc_point
            .set(self.total_alloc_point.get() + alloc_point);

        let mut new_pool = self.pool_info.grow();
        new_pool.lp_token.set(lp_token);
        new_pool.alloc_point.set(alloc_point);
        new_pool.last_reward_block.set(last_reward_block);
        new_pool.acc_sushi_per_share.set(U256::from(0));

        Ok(())
    }

    pub fn set(&mut self) -> Result<(), MasterChefError> {
        // onlyOwner modifier.
        if self.owner.get() != msg::sender() {
            return Err(MasterChefError::NonOwner(NonOwner {}));
        }

        Ok(())
    }

    pub fn set_migrator(&mut self, migrator: Address) -> Result<(), MasterChefError> {
        // onlyOwner modifier.
        if self.owner.get() != msg::sender() {
            return Err(MasterChefError::NonOwner(NonOwner {}));
        }

        self.migrator.set(migrator);
        Ok(())
    }

    // Update dev address by the previous dev.
    pub fn dev(&mut self, dev_addr: Address) -> Result<(), MasterChefError> {
        if self.dev_addr.get() != msg::sender() {
            return Err(MasterChefError::NotDevAddress(NotDevAddress {}));
        }

        self.dev_addr.set(dev_addr);
        Ok(())
    }
}

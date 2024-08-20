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
    block, contract, msg,
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
        function mint(address, uint256) external;
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
        uint256 bonus_multiplier;
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
    error PoolDoesNotExist();
}

#[derive(SolidityError)]
pub enum MasterChefError {
    AlreadyInitialized(AlreadyInitialized),
    NonOwner(NonOwner),
    NotDevAddress(NotDevAddress),
    PoolDoesNotExist(PoolDoesNotExist),
}

#[external]
impl MasterChef {
    pub fn pool_length(&self) -> U256 {
        U256::from(self.pool_info.len())
    }

    /// Initialize - Constructor.
    pub fn initialize(
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
        self.bonus_multiplier.set(U256::from(10));
        self.start_block.set(start_block);

        Ok(())
    }

    /// Admin functions

    /**
     * Add a new lp to the pool. Can only be called by the owner.
     * XXX DO NOT add the same LP token more than once. Rewards will be messed up if you do.
     */
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

    // Update the given pool's SUSHI allocation point. Can only be called by the owner.
    pub fn set(
        &mut self,
        pid: U256,
        alloc_point: U256,
        with_update: bool,
    ) -> Result<(), MasterChefError> {
        // onlyOwner modifier.
        if self.owner.get() != msg::sender() {
            return Err(MasterChefError::NonOwner(NonOwner {}));
        }

        if with_update {
            // massUpdatePools()
        }

        if let Some(mut pool_alloc_point) = self.pool_info.get_mut(pid) {
            self.total_alloc_point.set(
                self.total_alloc_point.get() - pool_alloc_point.alloc_point.get() + alloc_point,
            );
            pool_alloc_point.alloc_point.set(alloc_point);

            Ok(())
        } else {
            return Err(MasterChefError::PoolDoesNotExist(PoolDoesNotExist {}));
        }
    }

    // Set the migrator contract. Can only be called by the owner.
    pub fn set_migrator(&mut self, migrator: Address) -> Result<(), MasterChefError> {
        // onlyOwner modifier.
        if self.owner.get() != msg::sender() {
            return Err(MasterChefError::NonOwner(NonOwner {}));
        }

        self.migrator.set(migrator);
        Ok(())
    }

    // Return reward multiplier over the given _from to _to block.
    pub fn get_multiplier(&self, from: U256, to: U256) -> U256 {
        if to <= self.bonus_end_block.get() {
            (to - from) * self.bonus_multiplier.get()
        } else if from >= self.bonus_end_block.get() {
            to - from
        } else {
            (self.bonus_end_block.get() - from) * self.bonus_multiplier.get() + to
                - self.bonus_end_block.get()
        }
    }

    // View function to see pending SUSHIs on frontend.
    pub fn pending_sushi(&self, pid: U256, user: Address) -> U256 {
        let pool_info = if let Some(pool) = self.pool_info.getter(pid) {
            pool
        } else {
            return U256::from(0);
        };

        let mut acc_sushi_per_share = pool_info.acc_sushi_per_share.get();
        let lp_supply = IERC20::new(pool_info.lp_token.get())
            .balance_of(self, contract::address())
            .unwrap_or(U256::from(0));

        let user_info = self.user_info.get(pid);
        let user = user_info.get(user);

        let block_number = U256::from(block::number());
        if block_number > pool_info.last_reward_block.get() && lp_supply != U256::from(0) {
            let multiplier = self.get_multiplier(pool_info.last_reward_block.get(), block_number);
            let sushi_reward =
                multiplier * self.sushi_per_block.get() * pool_info.alloc_point.get()
                    / self.total_alloc_point.get();

            acc_sushi_per_share =
                acc_sushi_per_share + (sushi_reward * U256::from(1_000_000_000_000u64) / lp_supply);
        }

        return user.amount.get() * acc_sushi_per_share / U256::from(1_000_000_000_000u64)
            - user.reward_debt.get();
    }

    // Update reward vairables for all pools. Be careful of gas spending!
    // pub fn mass_update_pools(&self) {
    //     let pool_length = self.pool_info.len();
    //     for i in 0..pool_length {
    //         // self.update_pool(pid)
    //     }
    // }

    pub fn update_pool(&mut self, pid: U256) -> Result<(), MasterChefError> {
        let sushi_per_block = self.sushi_per_block.get();
        let total_alloc_point = self.total_alloc_point.get();
        let sushi_token_address = *self.sushi;
        let dev_addr = self.dev_addr.get();

        let lp_supply;
        let multiplier;
        let mut sushi_reward = U256::from(0);

        if let Some(pool) = self.pool_info.getter(pid) {
            if U256::from(block::number()) <= pool.last_reward_block.get() {
                return Ok(());
            }

            lp_supply = IERC20::new(pool.lp_token.get())
                .balance_of(&*self, contract::address())
                .unwrap_or(U256::from(0));

            multiplier =
                self.get_multiplier(pool.last_reward_block.get(), U256::from(block::number()));
        } else {
            return Err(MasterChefError::PoolDoesNotExist(PoolDoesNotExist {}));
        };

        if let Some(mut pool_info) = self.pool_info.get_mut(pid) {
            if lp_supply == U256::from(0) {
                pool_info.last_reward_block.set(U256::from(block::number()));
                return Ok(());
            }

            sushi_reward =
                multiplier * sushi_per_block * pool_info.alloc_point.get() / total_alloc_point;

            let acc_sushi_per_share = pool_info.acc_sushi_per_share.get()
                + (sushi_reward * U256::from(1_000_000_000_000u64) / lp_supply);

            pool_info.acc_sushi_per_share.set(acc_sushi_per_share);
            pool_info.last_reward_block.set(U256::from(block::number()));
        }

        let sushi_token = IERC20::new(sushi_token_address);

        let _ = sushi_token.mint(&mut *self, dev_addr, sushi_reward / U256::from(10));
        let _ = sushi_token.mint(self, contract::address(), sushi_reward);

        return Ok(());
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

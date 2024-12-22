#[macro_use]
extern crate serde;
use candid::{Decode, Encode, Nat, Principal};
use ic_cdk::api::{call, time};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

// Definisi token interface USDT
#[derive(candid::CandidType)]
struct TokenInterface {
    token_canister: Principal,
}

#[derive(candid::CandidType)]
struct TokenTransferArgs {
    to: Principal,
    value: Nat,
}

const USDT_CANISTER_ID: &str = "renrk-eyaaa-aaaaa-aaada-cai"; // Ganti dengan ID canister USDT yang sebenarnya

// Struktur untuk menyimpan data PDF SK
#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
struct PdfFile {
    content: Vec<u8>,
    is_verified: bool,
}

// Struktur untuk wallet
#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
struct Wallet {
    principal_id: Principal,
    balance: Nat,
}

// Struktur untuk menyimpan data attendance
#[derive(candid::CandidType, Clone, Serialize, Deserialize)]
struct Attendance {
    check_in: u64,
    check_out: u64,
    total_hours: f64,
    daily_wage: f64,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Employee {
    nip: u64,
    name: String,
    age: u32,
    pension_age: u32,
    wage_per_hour: f64,
    sk_file: Option<PdfFile>,
    wallet_address: String,
    created_at: u64,
    updated_at: Option<u64>,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct PayrollApproval {
    employee_nip: u64,
    attendance_date: u64,
    wage_amount: f64,
    status: ApprovalStatus,
    manager_wallet: String,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
enum ApprovalStatus {
    #[default]
    Pending,
    Approved,
    Rejected,
}

impl Storable for Employee {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Employee {
    const MAX_SIZE: u32 = 2048;
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static EMPLOYEE_STORAGE: RefCell<StableBTreeMap<u64, Employee, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))))
    );

    static ATTENDANCE_STORAGE: RefCell<StableBTreeMap<(u64, u64), Attendance, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2))))
    );

    static APPROVAL_STORAGE: RefCell<StableBTreeMap<(u64, u64), PayrollApproval, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(3))))
    );

    static WALLET_STORAGE: RefCell<StableBTreeMap<Principal, Wallet, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(4))))
    );
}

#[derive(candid::CandidType, Serialize, Deserialize)]
struct EmployeePayload {
    name: String,
    age: u32,
    wage_per_hour: f64,
    wallet_address: String,
}

// Fungsi Token Interface
fn get_token_interface() -> TokenInterface {
    TokenInterface {
        token_canister: Principal::from_text(USDT_CANISTER_ID).unwrap(),
    }
}

fn convert_wage_to_token_amount(wage_amount: f64) -> Nat {
    let token_amount = (wage_amount * 1_000_000.0) as u64;
    Nat::from(token_amount)
}

// Fungsi Perhitungan
fn calculate_pension_age(age: u32) -> u32 {
    60 - age
}

fn calculate_work_hours(check_in: u64, check_out: u64) -> f64 {
    let diff = check_out - check_in;
    (diff as f64) / (1000.0 * 60.0 * 60.0)
}

fn calculate_daily_wage(total_hours: f64, wage_per_hour: f64) -> f64 {
    total_hours * wage_per_hour
}

// CRUD Operations untuk Employee
#[ic_cdk::update]
fn add_employee(payload: EmployeePayload) -> Option<Employee> {
    let nip = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("cannot increment id counter");

    let pension_age = calculate_pension_age(payload.age);
    
    let employee = Employee {
        nip,
        name: payload.name,
        age: payload.age,
        pension_age,
        wage_per_hour: payload.wage_per_hour,
        sk_file: None,
        wallet_address: payload.wallet_address,
        created_at: time(),
        updated_at: None,
    };

    EMPLOYEE_STORAGE.with(|storage| storage.borrow_mut().insert(nip, employee.clone()));
    Some(employee)
}

#[ic_cdk::query]
fn get_employee(nip: u64) -> Result<Employee, Error> {
    EMPLOYEE_STORAGE.with(|storage| {
        storage.borrow().get(&nip)
            .ok_or(Error::NotFound {
                msg: format!("Employee with NIP={} not found", nip),
            })
    })
}

// Attendance Management
#[ic_cdk::update]
fn record_attendance(nip: u64, check_in: u64, check_out: u64) -> Result<Attendance, Error> {
    let employee = match EMPLOYEE_STORAGE.with(|storage| storage.borrow().get(&nip)) {
        Some(emp) => emp,
        None => return Err(Error::NotFound {
            msg: format!("Employee with NIP={} not found", nip),
        }),
    };

    let total_hours = calculate_work_hours(check_in, check_out);
    let daily_wage = calculate_daily_wage(total_hours, employee.wage_per_hour);

    let attendance = Attendance {
        check_in,
        check_out,
        total_hours,
        daily_wage,
    };

    let current_date = time() / (24 * 60 * 60 * 1_000_000_000);
    ATTENDANCE_STORAGE.with(|storage| 
        storage.borrow_mut().insert((nip, current_date), attendance.clone())
    );

    Ok(attendance)
}

// Wallet dan Transfer Functions
#[ic_cdk::update]
async fn transfer_usdt(from: Principal, to: Principal, amount: Nat) -> Result<(), String> {
    let token_interface = get_token_interface();
    
    let transfer_args = TokenTransferArgs {
        to,
        value: amount,
    };

    match call::call(
        token_interface.token_canister,
        "transfer",
        (transfer_args,),
    ).await {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to transfer USDT: {:?}", e)),
    }
}

#[ic_cdk::update]
fn register_wallet(principal: Principal) -> Result<Wallet, Error> {
    let wallet = Wallet {
        principal_id: principal,
        balance: Nat::from(0),
    };

    WALLET_STORAGE.with(|storage| {
        storage.borrow_mut().insert(principal, wallet.clone());
    });

    Ok(wallet)
}

#[ic_cdk::query]
fn get_wallet_balance(principal: Principal) -> Result<Nat, Error> {
    WALLET_STORAGE.with(|storage| {
        let storage = storage.borrow();
        let wallet = storage.get(&principal)
            .ok_or(Error::NotFound {
                msg: "Wallet not found".to_string(),
            })?;
        Ok(wallet.balance)
    })
}

// Approval Process
#[ic_cdk::update]
fn request_approval(nip: u64, manager_wallet: String) -> Result<PayrollApproval, Error> {
    let current_date = time() / (24 * 60 * 60 * 1_000_000_000);
    
    let attendance = match ATTENDANCE_STORAGE.with(|storage| 
        storage.borrow().get(&(nip, current_date))
    ) {
        Some(att) => att,
        None => return Err(Error::NotFound {
            msg: format!("Attendance for NIP={} on current date not found", nip),
        }),
    };

    let approval = PayrollApproval {
        employee_nip: nip,
        attendance_date: current_date,
        wage_amount: attendance.daily_wage,
        status: ApprovalStatus::Pending,
        manager_wallet,
    };

    APPROVAL_STORAGE.with(|storage| 
        storage.borrow_mut().insert((nip, current_date), approval.clone())
    );

    Ok(approval)
}

#[ic_cdk::update]
async fn approve_payroll(nip: u64, date: u64, approved: bool) -> Result<PayrollApproval, Error> {
    let approval_result = APPROVAL_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        let mut approval = storage.get(&(nip, date))
            .ok_or(Error::NotFound {
                msg: format!("Approval request for NIP={} on given date not found", nip),
            })?;

        if approved {
            approval.status = ApprovalStatus::Approved;
        } else {
            approval.status = ApprovalStatus::Rejected;
        }

        storage.insert((nip, date), approval.clone());
        Ok(approval)
    })?;

    if approved {
        let employee = EMPLOYEE_STORAGE.with(|storage| {
            storage.borrow().get(&approval_result.employee_nip)
        }).ok_or(Error::NotFound {
            msg: format!("Employee not found"),
        })?;

        let manager_principal = Principal::from_text(&approval_result.manager_wallet)
            .map_err(|_| Error::InvalidWallet {
                msg: "Invalid manager wallet".to_string(),
            })?;

        let employee_principal = Principal::from_text(&employee.wallet_address)
            .map_err(|_| Error::InvalidWallet {
                msg: "Invalid employee wallet".to_string(),
            })?;

        let token_amount = convert_wage_to_token_amount(approval_result.wage_amount);

        match transfer_usdt(manager_principal, employee_principal, token_amount).await {
            Ok(_) => {
                update_wallet_balance(employee_principal, token_amount)?;
            }
            Err(e) => {
                return Err(Error::TransferFailed {
                    msg: e,
                });
            }
        }
    }

    Ok(approval_result)
}

// Helper Functions
fn update_wallet_balance(principal: Principal, amount: Nat) -> Result<(), Error> {
    WALLET_STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        let mut wallet = storage.get(&principal)
            .unwrap_or(Wallet {
                principal_id: principal,
                balance: Nat::from(0),
            });

        wallet.balance += amount;
        storage.insert(principal, wallet);
        Ok(())
    })
}

fn validate_wallet(wallet_address: &str) -> Result<Principal, Error> {
    Principal::from_text(wallet_address)
        .map_err(|_| Error::InvalidWallet {
            msg: "Invalid wallet address format".to_string(),
        })
}

// Error Handling
#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
    InvalidWallet { msg: String },
    TransferFailed { msg: String },
}

// Export Candid interface
ic_cdk::export_candid!();

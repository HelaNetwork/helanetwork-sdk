package consensusaccounts

import "github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"

// Deposit are the arguments for consensus.Deposit method.
type Deposit struct {
	To     *types.Address  `json:"to,omitempty"`
    EthTo   [20]byte       `json:"eth_to,emitted"`
	Amount types.BaseUnits `json:"amount"`
}

// Withdraw are the arguments for consensus.Withdraw method.
type Withdraw struct {
    EthFrom [20]byte       `json:"eth_from,emitted"`
	To     *types.Address  `json:"to,omitempty"`
	Amount types.BaseUnits `json:"amount"`
}

// BalanceQuery are the arguments for consensus.Balance method.
type BalanceQuery struct {
	Address types.Address `json:"address"`
}

// AccountBalance is the consensus balance in an account.
type AccountBalance struct {
	Balance types.Quantity `json:"balance"`
}

// AccountQuery are the arguments for consensus.Account method.
type AccountQuery struct {
	Address types.Address `json:"address"`
}

// GasCosts are the consensus accounts module gas costs.
type GasCosts struct {
	TxDeposit  uint64 `json:"tx_deposit"`
	TxWithdraw uint64 `json:"tx_withdraw"`
}

// Parameters are the parameters for the consensus accounts module.
type Parameters struct {
	GasCosts GasCosts `json:"gas_costs"`
}

// ConsensusError contains error details from the consensus layer.
type ConsensusError struct {
	Module string `json:"module,omitempty"`
	Code   uint32 `json:"code,omitempty"`
}

// ModuleName is the consensus accounts module name.
const ModuleName = "consensus_accounts"

const (
	// DepositEventCode is the event code for the deposit event.
	DepositEventCode = 1
	// WithdrawEventCode is the event code for the withdraw event.
	WithdrawEventCode = 2
)

// DepositEvent is a deposit event.
type DepositEvent struct {
	From   types.Address   `json:"from"`
	Nonce  uint64          `json:"nonce"`
	To     types.Address   `json:"to"`
    EthTo  [20]byte        `json:"eth_to"`
	Amount types.BaseUnits `json:"amount"`
	Error  *ConsensusError `json:"error,omitempty"`
}

// IsSuccess checks whether the event indicates a successful operation.
func (de *DepositEvent) IsSuccess() bool {
	return de.Error == nil
}

// WithdrawEvent is a withdraw event.
type WithdrawEvent struct {
	From   types.Address   `json:"from"`
    EthFrom [20]byte       `json:"eth_from"`
	Nonce  uint64          `json:"nonce"`
	To     types.Address   `json:"to"`
	Amount types.BaseUnits `json:"amount"`
	Error  *ConsensusError `json:"error,omitempty"`
}

// IsSuccess checks whether the event indicates a successful operation.
func (we *WithdrawEvent) IsSuccess() bool {
	return we.Error == nil
}

// Event is a consensus account event.
type Event struct {
	Deposit  *DepositEvent
	Withdraw *WithdrawEvent
}

package accounts

import (
	"context"
	"fmt"

	"github.com/oasisprotocol/oasis-core/go/common/cbor"

	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/client"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

const (
	// Callable methods.
	methodTransfer = "accounts.Transfer"

	// GB: InitOwners.
	methodInitOwners = "accounts.InitOwners"
	methodPropose    = "accounts.Propose"
	methodVoteST     = "accounts.VoteST"

	// GB: insert methodMintST for MintST and methodBurnST.
	methodMintST = "accounts.MintST"
	methodBurnST = "accounts.BurnST"

	// Queries.
	methodParameters = "accounts.Parameters"
	methodNonce      = "accounts.Nonce"
	// GB: insert for role and init status of account inquiry.
	methodRole         = "accounts.Role"
	methodInit         = "accounts.Init"
	methodBlacklist    = "accounts.Blacklisted"
	methodQuorum    = "accounts.Quorum"

	methodRoleAddresses        = "accounts.RoleAddresses"
	methodProposalID   = "accounts.ProposalID"
	methodProposalInfo = "accounts.ProposalInfo"

	methodBalances         = "accounts.Balances"
	methodAddresses        = "accounts.Addresses"
	methodDenominationInfo = "accounts.DenominationInfo"
)

// This interface seems defined for testing or web3?
// as transactions from command line are already wrapped by types.NewTransactions
// V1 is the v1 accounts module interface.
type V1 interface {
	client.EventDecoder

	// Transfer generates an accounts.Transfer transaction.
	Transfer(to types.Address, amount types.BaseUnits) *client.TransactionBuilder

	// GB: InitOwners generates an accounts.InitOwners transaction.
	// InitOwners(addr types.Address, role types.Role) *client.TransactionBuilder
	// InitOwners(accounts map[types.Address]types.Role) *client.TransactionBuilder

	// GB: MintST generates an accounts.MintST transaction.
	MintST(to types.Address, amount types.BaseUnits) *client.TransactionBuilder
	BurnST(amount types.BaseUnits) *client.TransactionBuilder

	// Parameters queries the accounts module parameters.
	Parameters(ctx context.Context, round uint64) (*Parameters, error)

	// Nonce queries the given account's nonce.
	Nonce(ctx context.Context, round uint64, address types.Address) (uint64, error)

	Role(ctx context.Context, round uint64, address types.Address) (types.Role, error)
	InitInfo(ctx context.Context, round uint64, address types.Address) (bool, error)
	Blacklist(ctx context.Context, round uint64, address types.Address) (bool, error)
	Quorums(ctx context.Context, round uint64, action types.Action) (uint8, error)
	RolesTeam(ctx context.Context, round uint64, role types.Role) ([]types.Address, error)
	ProposalIDInfo(ctx context.Context, round uint64) (uint32, error)
	ProposalInfo(ctx context.Context, round uint64, id uint32) (*ProposalOutput, error)

	// Balances queries the given account's balances.
	Balances(ctx context.Context, round uint64, address types.Address) (*AccountBalances, error)

	// Addresses queries all account addresses.
	Addresses(ctx context.Context, round uint64, denomination types.Denomination) (Addresses, error)

	// DenominationInfo queries the information about a given denomination.
	DenominationInfo(ctx context.Context, round uint64, denomination types.Denomination) (*DenominationInfo, error)

	// GetEvents returns all account events emitted in a given block.
	GetEvents(ctx context.Context, round uint64) ([]*Event, error)
}

type v1 struct {
	rc client.RuntimeClient
}

// Implements V1.
func (a *v1) Transfer(to types.Address, amount types.BaseUnits) *client.TransactionBuilder {
	return client.NewTransactionBuilder(a.rc, methodTransfer, &Transfer{
		To:     to,
		Amount: amount,
	})
}

// GB: Implements V1 for InitOwners, this one is for mutiple accounts with roles.
// func (a *v1) InitOwners(accounts map[types.Address]types.Role) *client.TransactionBuilder {
// 	roleAddrs := make([]*RoleAddress, 0, len(accounts))
// 	for addr, role := range accounts {
// 		roleAddrs = append(roleAddrs, &RoleAddress{
// 			Addr: addr,
// 			Role: role,
// 		})
// 	}
// 	return client.NewTransactionBuilder(a.rc, methodInitOwners, roleAddrs)
// }

// GB: Implements V1 for Propose mint/burn/blacklist etc.
// func (a *v1) Propose(
// 	id uint32,
// 	submitter types.Address,
// 	state types.ProposalState,
// 	content *types.ProposalContent,
// 	results map[types.Vote]uint16,
// 	invalidVotes *uint16
// ) *client.TransactionBuilder {

// 	proposal := &Proposal{
// 		ID:           id,
// 		Submitter:    submitter,
// 		State:        state,
// 		Content:      content,
// 		Results:      results,
// 		InvalidVotes: invalidVotes,
// 	}

// 	return client.NewTransactionBuilder(a.rc, methodPropose, proposal)
// }

// GB: Implements V1 for MintST and BurnST
func (a *v1) MintST(to types.Address, amount types.BaseUnits) *client.TransactionBuilder {
	return client.NewTransactionBuilder(a.rc, methodMintST, &MintST{
		To:     to,
		Amount: amount,
	})
}

func (a *v1) BurnST(amount types.BaseUnits) *client.TransactionBuilder {
	return client.NewTransactionBuilder(a.rc, methodBurnST, &BurnST{
		// To:     to,
		Amount: amount,
	})
}

// Implements V1.
func (a *v1) Parameters(ctx context.Context, round uint64) (*Parameters, error) {
	var params Parameters
	err := a.rc.Query(ctx, round, methodParameters, nil, &params)
	if err != nil {
		return nil, err
	}
	return &params, nil
}

// Implements V1.
func (a *v1) Nonce(ctx context.Context, round uint64, address types.Address) (uint64, error) {
	var nonce uint64
	err := a.rc.Query(ctx, round, methodNonce, &NonceQuery{Address: address}, &nonce)
	if err != nil {
		return 0, err
	}
	return nonce, nil
}

// GB: Implements V1 for role of account
func (a *v1) Role(ctx context.Context, round uint64, address types.Address) (types.Role, error) {
	var role types.Role
	err := a.rc.Query(ctx, round, methodRole, &RoleQuery{Address: address}, &role)
	if err != nil {
		return 0, err
	}
	return role, nil
}

// GB: Implements V1 for init status of account
func (a *v1) InitInfo(ctx context.Context, round uint64, address types.Address) (bool, error) {
	var init bool
	err := a.rc.Query(ctx, round, methodInit, &InitInfoQuery{Address: address}, &init)
	if err != nil {
		return false, err
	}
	return init, nil
}

// Sifei: Implements V1 for blacklist of account
func (a *v1) Blacklist(ctx context.Context, round uint64, address types.Address) (bool, error) {
	var blacklist bool
	err := a.rc.Query(ctx, round, methodBlacklist, &BlacklistQuery{Address: address}, &blacklist)
	if err != nil {
		return false, err
	}
	return blacklist, nil
}


func (a *v1) RolesTeam(ctx context.Context, round uint64, role types.Role) ([]types.Address, error) {
	var addresses []types.Address
	err := a.rc.Query(ctx, round, methodRoleAddresses, &RoleAddressesQuery{Role: role}, &addresses)
	if err != nil {
		return nil, err
	}
	return addresses, nil
}

func (a *v1) Quorums(ctx context.Context, round uint64, action types.Action) (uint8, error) {
	var quorum_no uint8
	err := a.rc.Query(ctx, round, methodQuorum, &QuorumsQuery{Action: action}, &quorum_no)
	if err != nil {
		return 0, err
	}
	return quorum_no, nil
}


func (a *v1) ProposalIDInfo(ctx context.Context, round uint64) (uint32, error) {
	var id uint32
	err := a.rc.Query(ctx, round, methodProposalID, nil, &id)
	if err != nil {
		return 0, err
	}
	return id, nil
}

func (a *v1) ProposalInfo(ctx context.Context, round uint64, id uint32) (*ProposalOutput, error) {
	var proposalOutput ProposalOutput
	err := a.rc.Query(ctx, round, methodProposalInfo, &id, &proposalOutput)
	if err != nil {
		return nil, err
	}
	return &proposalOutput, nil
}

// Implements V1.
func (a *v1) Balances(ctx context.Context, round uint64, address types.Address) (*AccountBalances, error) {
	var balances AccountBalances
	err := a.rc.Query(ctx, round, methodBalances, &BalancesQuery{Address: address}, &balances)
	if err != nil {
		return nil, err
	}
	return &balances, nil
}

// Implements V1.
func (a *v1) Addresses(ctx context.Context, round uint64, denomination types.Denomination) (Addresses, error) {
	var addresses Addresses
	err := a.rc.Query(ctx, round, methodAddresses, &AddressesQuery{Denomination: denomination}, &addresses)
	if err != nil {
		return nil, err
	}
	return addresses, nil
}

// Implements V1.
func (a *v1) DenominationInfo(ctx context.Context, round uint64, denomination types.Denomination) (*DenominationInfo, error) {
	var info DenominationInfo
	err := a.rc.Query(ctx, round, methodDenominationInfo, &DenominationInfoQuery{Denomination: denomination}, &info)
	if err != nil {
		return nil, err
	}
	return &info, nil
}

// Implements V1.
func (a *v1) GetEvents(ctx context.Context, round uint64) ([]*Event, error) {
	rawEvs, err := a.rc.GetEventsRaw(ctx, round)
	if err != nil {
		return nil, err
	}

	evs := make([]*Event, 0)
	for _, rawEv := range rawEvs {
		ev, err := a.DecodeEvent(rawEv)
		if err != nil {
			return nil, err
		}
		for _, e := range ev {
			evs = append(evs, e.(*Event))
		}
	}

	return evs, nil
}

// Implements client.EventDecoder.
func (a *v1) DecodeEvent(event *types.Event) ([]client.DecodedEvent, error) {
	return DecodeEvent(event)
}

// DecodeEvent decodes an accounts event.
func DecodeEvent(event *types.Event) ([]client.DecodedEvent, error) {
	if event.Module != ModuleName {
		return nil, nil
	}
	var events []client.DecodedEvent
	switch event.Code {
	case TransferEventCode:
		var evs []*TransferEvent
		if err := cbor.Unmarshal(event.Value, &evs); err != nil {
			return nil, fmt.Errorf("decode account transfer event value: %w", err)
		}
		for _, ev := range evs {
			events = append(events, &Event{Transfer: ev})
		}
	case BurnEventCode:
		var evs []*BurnEvent
		if err := cbor.Unmarshal(event.Value, &evs); err != nil {
			return nil, fmt.Errorf("decode account burn event value: %w", err)
		}
		for _, ev := range evs {
			events = append(events, &Event{Burn: ev})
		}
	case MintEventCode:
		var evs []*MintEvent
		if err := cbor.Unmarshal(event.Value, &evs); err != nil {
			return nil, fmt.Errorf("decode account mint event value: %w", err)
		}
		for _, ev := range evs {
			events = append(events, &Event{Mint: ev})
		}
	// GBTODO: may need to insert MintSTEventCode here.
	default:
		return nil, fmt.Errorf("invalid accounts event code: %v", event.Code)
	}
	return events, nil
}

// NewV1 generates a V1 client helper for the accounts module.
func NewV1(rc client.RuntimeClient) V1 {
	return &v1{rc: rc}
}

// NewTransferTx generates a new accounts.Transfer transaction.
func NewTransferTx(fee *types.Fee, body *Transfer) *types.Transaction {
	return types.NewTransaction(fee, methodTransfer, body)
}

// GB: NewInitOwnersTx generates a new accounts.InitOwners transaction.
func NewInitOwnersTx(fee *types.Fee, body []RoleAddress) *types.Transaction {
	return types.NewTransaction(fee, methodInitOwners, body)
}

func NewProposeTx(fee *types.Fee, body *ProposalContent) *types.Transaction {
	return types.NewTransaction(fee, methodPropose, body)
}

func NewVoteSTTx(fee *types.Fee, body *VoteProposal) *types.Transaction {
	return types.NewTransaction(fee, methodVoteST, body)
}

// GB: NewMintSTTx generates a new accounts.MintST transaction.
func NewMintSTTx(fee *types.Fee, body *MintST) *types.Transaction {
	return types.NewTransaction(fee, methodMintST, body)
}

// GB: NewBurnSTTx generates a new accounts.BurnST transaction.
func NewBurnSTTx(fee *types.Fee, body *BurnST) *types.Transaction {
	return types.NewTransaction(fee, methodBurnST, body)
}

package types

import (
	"fmt"
	"unsafe"
)

type ProposalState uint8

const (
	Active ProposalState = iota
	Passed
	Rejected
	Expired
	Cancelled
)

const ProposalStateSize = int(unsafe.Sizeof(ProposalState(0)))

func (ps ProposalState) MarshalBinary() []byte {
	return []byte{byte(ps)}
}

func (ps *ProposalState) UnmarshalBinary(data []byte) error {
	if len(data) != ProposalStateSize {
		return fmt.Errorf("Fail to decode proposalState")
	}

	*ps = ProposalState(data[0])
	return nil
}

func (ps ProposalState) String() string {
	switch ps {
	case Active:
		return "Active"
	case Passed:
		return "Passed"
	case Rejected:
		return "Rejected"
	case Expired:
		return "Expired"
	case Cancelled:
		return "Cancelled"
	default:
		return "Unknown"
	}
}

const MaxMeta = 64

type Meta [MaxMeta]byte

// MarshalBinary encodes meta into binary form.
func (m *Meta) MarshalBinary() (data []byte, err error) {
	if m == nil {
		return []byte{}, nil
	}

	data = append([]byte{}, m[:]...)
	return
}

// UnmarshalBinary decodes a binary marshaled meta.
func (m *Meta) UnmarshalBinary(data []byte) error {
	// fmt.Printf("gbtest: go into Meta UnmarshalBinary\n")
	if len(data) != MaxMeta {
		return fmt.Errorf("Fail to decode Meta: input data size exceeds MaxMeta")
	}

	copy(m[:], data)

	return nil
}

func StringToMeta(s *string) (*Meta, error) {
	if s == nil {
		return nil, nil
	}

	if len(*s) > MaxMeta {
		return nil, fmt.Errorf("input string is too long, maximum length allowed is %d", MaxMeta)
	}

	var meta Meta
	for i := 0; i < len(*s); i++ {
		meta[i] = (*s)[i]
	}
	return &meta, nil
}

type ProposalData struct {
	Address         *Address   `json:"address,omitempty"`
	Amount          *BaseUnits `json:"amount,omitempty"`
	Meta            *Meta      `json:"meta,omitempty"`
	Role            *Role      `json:"role,omitempty"`
	MintQuorum      *uint8    `json:"mint_quorum,omitempty"`
	BurnQuorum      *uint8    `json:"burn_quorum,omitempty"`
	WhitelistQuorum *uint8    `json:"whitelist_quorum,omitempty"`
	BlacklistQuorum *uint8    `json:"blacklist_quorum,omitempty"`
	ConfigQuorum    *uint8    `json:"config_quorum,omitempty"`
}

type ProposalDataStr struct {
	Address         *string `json:"address"`
	Amount          *string `json:"amount"`
	Meta            *string `json:"meta"`
	Role            *string `json:"role"`
	MintQuorum      *uint8 `json:"mint_quorum"`
	BurnQuorum      *uint8 `json:"burn_quorum"`
	WhitelistQuorum *uint8 `json:"whitelist_quorum"`
	BlacklistQuorum *uint8 `json:"blacklist_quorum"`
	ConfigQuorum    *uint8 `json:"config_quorum"`
}

func (pd *ProposalData) String(action Action) (map[string]string, error) {
	result := make(map[string]string)

	switch action {
	case SetRoles:
		if pd.Role == nil || pd.Address == nil {
			return nil, fmt.Errorf("Failed to output %s.", action.String())
		}

		result["Role"] = pd.Role.String()
		result["Address"] = pd.Address.String()

	case Whitelist, Blacklist:
		if pd.Address == nil {
			return nil, fmt.Errorf("Failed to output %s.", action.String())
		}

		result["Address"] = pd.Address.String()

	case Mint, Burn:
		if pd.Address == nil || pd.Amount == nil {
			return nil, fmt.Errorf("Failed to output %s.", action.String())
		}

		result["Address"] = pd.Address.String()
		result["Amount"] = pd.Amount.String()

	case Config:
		if pd.MintQuorum == nil && pd.BurnQuorum == nil && pd.WhitelistQuorum == nil && pd.BlacklistQuorum == nil && pd.ConfigQuorum == nil {
			return nil, fmt.Errorf("Failed to output Config.")
		}

		if pd.MintQuorum != nil {
			result["MintQuorum"] = fmt.Sprintf("%d", *pd.MintQuorum)
		}
		if pd.BurnQuorum != nil {
			result["BurnQuorum"] = fmt.Sprintf("%d", *pd.BurnQuorum)
		}
		if pd.WhitelistQuorum != nil {
			result["WhiteListQuorum"] = fmt.Sprintf("%d", *pd.WhitelistQuorum)
		}
		if pd.BlacklistQuorum != nil {
			result["BlacklistQuorum"] = fmt.Sprintf("%d", *pd.BlacklistQuorum)
		}
		if pd.ConfigQuorum != nil {
			result["ConfigQuorum"] = fmt.Sprintf("%d", *pd.ConfigQuorum)
		}
	}
	return result, nil
}
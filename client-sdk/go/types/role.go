package types

import (
	// "encoding/binary"
	"fmt"
	"strings"
	"unsafe"
)

// GB: The following is for input roles by accountsInitOwnersCmd
// by default, the roles should be within 255 although the type is uint16.
type Role uint8

// RoleSize is the size of Role in bytes.
const RoleSize = int(unsafe.Sizeof(Role(0)))

const (
	// Admins have complete control over the multi-signature wallet. They can add, remove, and manage authorized parties and their roles.
	Admin Role = iota

	MintProposer
	MintVoter
	BurnProposer
	BurnVoter
	WhitelistProposer
	WhitelistVoter
	BlacklistProposer
	BlacklistVoter

	WhitelistedUser
	BlacklistedUser

	User
)

func RoleFromString(roleStr string) (Role, error) {
	switch strings.ToLower(roleStr) {
	case "admin":
		return Admin, nil
	case "mint_proposer":
		return MintProposer, nil
	case "mint_voter":
		return MintVoter, nil
	case "burn_proposer":
		return BurnProposer, nil
	case "burn_voter":
		return BurnVoter, nil
	case "whitelist_proposer":
		return WhitelistProposer, nil
	case "whitelist_voter":
		return WhitelistVoter, nil
	case "blacklist_proposer":
		return BlacklistProposer, nil
	case "blacklist_voter":
		return BlacklistVoter, nil
	case "whitelisted_user":
		return WhitelistedUser, nil
	case "blacklisted_user":
		return BlacklistedUser, nil
	case "user":
		return User, nil
	default:
		return User, fmt.Errorf("unknown role: %s", roleStr)
	}
}

// GBTODO: it's better change this MarshalBinary/UnmarshalBinary methods to encode as integel.
// GB: These 2 functions are necessary, oasis-cbor doesn't handle encoding and decoding of uint8 by default.
func (r Role) MarshalBinary() ([]byte, error) {
	return []byte{byte(r)}, nil
}

// GB: the following 2 functions UnmarshalBinary/String are used to output info from the system.
func (r *Role) UnmarshalBinary(data []byte) error {
	if len(data) != RoleSize {
		return fmt.Errorf("Fail to decode in role")
	}
	*r = Role(data[0])
	return nil
}

func (r Role) String() string {
	switch r {
	case Admin:
		return "Admin"
	case MintProposer:
		return "MintProposer"
	case MintVoter:
		return "MintVoter"
	case BurnProposer:
		return "BurnProposer"
	case BurnVoter:
		return "BurnVoter"
	case WhitelistProposer:
		return "WhitelistProposer"
	case WhitelistVoter:
		return "WhitelistVoter"
	case BlacklistProposer:
		return "BlacklistProposer"
	case BlacklistVoter:
		return "BlacklistVoter"
	case WhitelistedUser:
		return "Whitelisted_User"
	case BlacklistedUser:
		return "Blacklisted_User"
	case User:
		return "User"
	default:
		return fmt.Sprintf("Unknown Role: %d", r)
	}
}







// GB: define action to uniquely limit the users' conducts.
type Action uint8

const (
	// keep mint/burn/whitelist/blacklist get the same value as role.
	NoAction Action = iota
	SetRoles
	Mint
	Burn
	Whitelist
	Blacklist
	Config
)

const ActionSize = int(unsafe.Sizeof(Action(0)))

func ActionFromString(actionStr string) (Action, error) {
	switch strings.ToLower(actionStr) {
	case "setroles":
		return SetRoles, nil
	case "mint":
		return Mint, nil
	case "burn":
		return Burn, nil
	case "whitelist":
		return Whitelist, nil
	case "blacklist":
		return Blacklist, nil
	case "config":
		return Config, nil
	default:
		return 0, fmt.Errorf("illegal action input!")
	}
}

func (a Action) MarshalBinary() ([]byte, error) {
	return []byte{byte(a)}, nil
}

// GB: the following 2 functions UnmarshalBinary/String are used to output info from the system.
func (a *Action) UnmarshalBinary(data []byte) error {
	if len(data) != ActionSize {
		return fmt.Errorf("Fail to decode in Action")
	}
	*a = Action(data[0])
	return nil
}

func (a Action) String() string {
	switch a {
	case NoAction:
		return "NoAction"
	case SetRoles:
		return "SetRoles"
	case Mint:
		return "Mint"
	case Burn:
		return "Burn"
	case Whitelist:
		return "Whitelist"
	case Blacklist:
		return "Blacklist"
	case Config:
		return "Config"
	default:
		return fmt.Sprintf("Unknown action: %d", a)
	}
}

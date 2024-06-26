package types

import (
    "unsafe"
    "fmt"
    "github.com/oasisprotocol/oasis-core/go/common/cbor"
)

type Vote uint8

const (
    VoteYes Vote = iota
    VoteNo
    VoteAbstain
)

const VoteSize = int(unsafe.Sizeof(Vote(0)))


func (v Vote) MarshalCBOR() ([]byte, error) {
    return cbor.Marshal(uint8(v)), nil
}

func (v *Vote) UnmarshalCBOR(data []byte) error {
    var decodedValue uint64
    err := cbor.Unmarshal(data, &decodedValue)
    if err != nil {
        return fmt.Errorf("Fail to decode vote: %v", err)
    }

    if decodedValue > 0xFF {
        return fmt.Errorf("Invalid vote value: %v", decodedValue)
    }

    *v = Vote(uint8(decodedValue))

    return nil
}

func (v Vote) String() string {
    switch v {
    case VoteYes:
        return "Yes"
    case VoteNo:
        return "No"
    case VoteAbstain:
        return "Abstain"
    default:
        return fmt.Sprintf("Unknown vote: %d", v)
    }
}

func StringToVote(s string) (Vote, error) {    
    switch s {
    case "yes":
        return VoteYes, nil
    case "no":
        return VoteNo, nil
    case "abstain":
        return VoteAbstain, nil
    default:
        return 0, fmt.Errorf("Invalid vote!")
    }
}
import * as oasis from '@oasisprotocol/client';

import * as event from './event';
import * as transaction from './transaction';
import * as types from './types';
import * as wrapper from './wrapper';

/**
 * Unique module name.
 */
export const MODULE_NAME = 'accounts';

export const ERR_INVALID_ARGUMENT_CODE = 1;
export const ERR_INSUFFICIENT_BALANCE_CODE = 2;
export const ERR_FORBIDDEN_CODE = 3;
export const ERR_NOT_FOUND_CODE = 4;

// Callable methods.
export const METHOD_TRANSFER = 'accounts.Transfer';

export const METHOD_INITOWNERS = "accounts.InitOwners"

// GB: insert METHOD_MINTST and METHOD_BURNST.
export const METHOD_MINTST = 'accounts.MintST';
export const METHOD_BURNST = 'accounts.BurnST';

// Queries.
export const METHOD_NONCE = 'accounts.Nonce';
export const METHOD_BALANCES = 'accounts.Balances';
export const METHOD_ADDRESSES = 'accounts.Addresses';
export const METHOD_DENOMINATION_INFO = 'accounts.DenominationInfo';

export const EVENT_TRANSFER_CODE = 1;
export const EVENT_BURN_CODE = 2;
export const EVENT_MINT_CODE = 3;

export class Wrapper extends wrapper.Base {
    constructor(runtimeID: Uint8Array) {
        super(runtimeID);
    }
    
    callTransfer() {
        return this.call<types.AccountsTransfer, void>(METHOD_TRANSFER);
    }

    // GB: insert METHOD_MINTST and METHOD_BURNST
    callMintST() {
        return this.call<types.AccountsMintST, void>(METHOD_MINTST);
    }
    callBurnST() {
        return this.call<types.AccountsBurnST, void>(METHOD_BURNST);
    }

    queryNonce() {
        return this.query<types.AccountsNonceQuery, oasis.types.longnum>(METHOD_NONCE);
    }

    queryBalances() {
        return this.query<types.AccountsBalancesQuery, types.AccountsAccountBalances>(
            METHOD_BALANCES,
        );
    }

    queryAddresses() {
        return this.query<types.AccountsAddressesQuery, Uint8Array[]>(METHOD_ADDRESSES);
    }

    queryDenominationInfo() {
        return this.query<types.AccountsDenominationInfoQuery, types.AccountsDenominationInfo>(
            METHOD_DENOMINATION_INFO,
        );
    }
}

export function moduleEventHandler(codes: {
    [EVENT_TRANSFER_CODE]?: event.Handler<types.AccountsTransferEvent>;
    [EVENT_BURN_CODE]?: event.Handler<types.AccountsBurnEvent>;
    [EVENT_MINT_CODE]?: event.Handler<types.AccountsMintEvent>;
}) {
    return [MODULE_NAME, codes] as event.ModuleHandler;
}

/**
 * Use this as a part of a {@link transaction.CallHandlers}.
 */
export type TransactionCallHandlers = {
    [METHOD_TRANSFER]?: transaction.CallHandler<types.AccountsTransfer>;
    [METHOD_MINTST]?: transaction.CallHandler<types.AccountsMintST>;
    [METHOD_BURNST]?: transaction.CallHandler<types.AccountsBurnST>;
};

// GB: insert METHOD_MINTST and METHOD_BURNST
// export type TransactionCallHandlers_Mintst = {
//     [METHOD_MINTST]?: transaction.CallHandler<types.AccountsMintST>;
// };

// export type TransactionCallHandlers_Burnst = {
//     [METHOD_BURNST]?: transaction.CallHandler<types.AccountsBurnST>;
// };
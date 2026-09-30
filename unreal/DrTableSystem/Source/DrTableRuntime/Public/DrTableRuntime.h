// Copyright ldnjin81. All Rights Reserved.

#pragma once

// Lookup contract used by code generated with `drtable build --runtime-header DrTableRuntime.h`
// (or `--ue-plugin`). Generated FMyRow::Find / FindBy<SubKey> / GetAll / Get<RefField> call these.
// Every function returns nullptr / empty when no registry is active or the table is not loaded.

#include "DrTableRegistry.h"

namespace DrTableRuntime
{
	template <typename TRow, typename TKey>
	const TRow* FindByKey(const TKey& Key)
	{
		const UDrTableRegistry* Registry = UDrTableRegistry::Get();
		return Registry ? Registry->FindRowByKey<TRow>(Key) : nullptr;
	}

	template <typename TRow, typename TKey>
	TArray<const TRow*> FindAllBySubKey(FName SubKeyName, const TKey& Key)
	{
		const UDrTableRegistry* Registry = UDrTableRegistry::Get();
		return Registry ? Registry->FindRowsBySubKey<TRow>(SubKeyName, Key) : TArray<const TRow*>();
	}

	template <typename TRow>
	TConstArrayView<TRow> GetAll()
	{
		const UDrTableRegistry* Registry = UDrTableRegistry::Get();
		return Registry ? Registry->GetRows<TRow>() : TConstArrayView<TRow>();
	}
} // namespace DrTableRuntime

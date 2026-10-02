// Copyright ldnjin81. All Rights Reserved.

#pragma once

// Lookup contract used by code generated with `drtable build --runtime-header DrTableRuntime.h`
// (or `--ue-plugin`). Generated FMyRow::Find / FindBy<SubKey> / GetAll / Get<RefField> / Get<ArrayField> call these.
// Every function returns nullptr / empty when no registry is active or the table is not loaded.

#include "DrStringTables.h"
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

	/** Elements of a row's array field: its run [Start, Start + Count) in the table's pool (generated Get<ArrayField>()). */
	template <typename TRow, typename TElement>
	TConstArrayView<TElement> GetArray(FName Field, int32 Start, int32 Count)
	{
		const UDrTableRegistry* Registry = UDrTableRegistry::Get();
		return Registry ? Registry->GetArray<TRow, TElement>(Field, Start, Count) : TConstArrayView<TElement>();
	}

	/** Text of a string table key in the current language (generated Ref<StringTable> accessors). */
	inline FText GetText(FName Table, FName Key)
	{
		return UDrStringTables::FindText(Table, Key);
	}
} // namespace DrTableRuntime

// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "Algo/BinarySearch.h"
#include "CoreMinimal.h"
#include "Engine/DataAsset.h"
#include "String/LexFromString.h"
#include <type_traits>

DRTABLERUNTIME_API DECLARE_LOG_CATEGORY_EXTERN(LogDrTable, Log, All);

namespace DrTable::Private
{
	/**
	 * Identity of a key type that is stable across modules (DLLs).
	 *
	 * An address of a function-local static in a header template is NOT stable: on
	 * Windows every module gets its own copy, so a lookup compiled in one module would
	 * never match a table registered from another. Enums use their UEnum, the other
	 * key types use fixed tags.
	 */
	template <typename TKey>
	const void* KeyTypeId()
	{
		if constexpr (std::is_enum_v<TKey>)
		{
			return StaticEnum<TKey>();
		}
		else if constexpr (std::is_same_v<TKey, FName>)
		{
			return reinterpret_cast<const void*>(UPTRINT(1));
		}
		else if constexpr (std::is_same_v<TKey, int32>)
		{
			return reinterpret_cast<const void*>(UPTRINT(2));
		}
		else if constexpr (std::is_same_v<TKey, int64>)
		{
			return reinterpret_cast<const void*>(UPTRINT(3));
		}
		else
		{
			static_assert(std::is_same_v<TKey, void>, "Table keys must be int32, int64, FName or a UENUM.");
			return nullptr;
		}
	}

	template <typename TKey>
	bool ConvertPrimaryKey(FName Name, TKey& OutKey)
	{
		if constexpr (std::is_same_v<TKey, FName>)
		{
			OutKey = Name;
			return true;
		}
		else if constexpr (std::is_enum_v<TKey>)
		{
			const UEnum* Enum = StaticEnum<TKey>();
			if (!Enum)
			{
				return false;
			}
			const int64 Value = Enum->GetValueByNameString(Name.ToString());
			if (Value == INDEX_NONE)
			{
				return false;
			}
			OutKey = static_cast<TKey>(Value);
			return true;
		}
		else if constexpr (std::is_arithmetic_v<TKey>)
		{
			const FString Value = Name.ToString();
			if (!Value.IsNumeric())
			{
				return false;
			}
			LexFromString(OutKey, FStringView(Value));
			return true;
		}
		else
		{
			static_assert(std::is_same_v<TKey, void>, "Unsupported primary key type.");
			return false;
		}
	}

	/**
	 * Ordering contract shared with the generator. drtable sorts keys:
	 * numbers numerically, names by Unicode code point (case-sensitive) and
	 * enums by their numeric value. This comparison must match it exactly, otherwise the
	 * binary search misses silently.
	 */
	template <typename TKey>
	bool KeyLess(const TKey& Left, const TKey& Right)
	{
		if constexpr (std::is_same_v<TKey, FName>)
		{
			return Left.ToString().Compare(Right.ToString(), ESearchCase::CaseSensitive) < 0;
		}
		else if constexpr (std::is_enum_v<TKey>)
		{
			return static_cast<std::underlying_type_t<TKey>>(Left) < static_cast<std::underlying_type_t<TKey>>(Right);
		}
		else
		{
			static_assert(std::is_arithmetic_v<TKey>, "Table keys must be arithmetic, an enum or FName.");
			return Left < Right;
		}
	}

	class FPrimaryKeyViewBase
	{
	public:
		virtual ~FPrimaryKeyViewBase() = default;
		virtual const void* GetKeyTypeId() const = 0;
		virtual bool Bind(const UPrimaryDataAsset& Asset) = 0;
		virtual int32 Find(FName Key) const = 0;
		virtual int32 FindTyped(const void* Key) const = 0;
		virtual void Reset() = 0;
	};

	template <typename TAsset, typename TKey>
	class TPrimaryKeyView final : public FPrimaryKeyViewBase
	{
	public:
		explicit TPrimaryKeyView(TArray<TKey> TAsset::*InKeysMember)
			: KeysMember(InKeysMember)
		{
		}

		virtual const void* GetKeyTypeId() const override { return KeyTypeId<TKey>(); }

		virtual bool Bind(const UPrimaryDataAsset& Asset) override
		{
			const TAsset* TypedAsset = Cast<TAsset>(&Asset);
			if (!TypedAsset)
			{
				return false;
			}
			Keys = &(TypedAsset->*KeysMember);
			return true;
		}

		virtual int32 Find(FName Key) const override
		{
			TKey ConvertedKey{};
			return ConvertPrimaryKey(Key, ConvertedKey) ? FindTyped(&ConvertedKey) : INDEX_NONE;
		}

		virtual int32 FindTyped(const void* Key) const override
		{
			if (!Keys || !Key)
			{
				return INDEX_NONE;
			}
			const TKey& TypedKey = *static_cast<const TKey*>(Key);
			const int32 Index = Algo::LowerBound(*Keys, TypedKey, KeyLess<TKey>);
			return Keys->IsValidIndex(Index) && (*Keys)[Index] == TypedKey ? Index : INDEX_NONE;
		}

		virtual void Reset() override { Keys = nullptr; }

	private:
		TArray<TKey> TAsset::*KeysMember;
		const TArray<TKey>* Keys = nullptr;
	};

	class FSubKeyViewBase
	{
	public:
		virtual ~FSubKeyViewBase() = default;
		virtual const void* GetKeyTypeId() const = 0;
		virtual bool Bind(const UPrimaryDataAsset& Asset) = 0;
		virtual TConstArrayView<int32> Find(const void* Key) const = 0;
		virtual void Reset() = 0;
	};

	/** Compressed sparse row index baked by the generator: sorted keys, bucket offsets, row indices. */
	template <typename TAsset, typename TKey>
	class TSubKeyView final : public FSubKeyViewBase
	{
	public:
		TSubKeyView(FName InName, TArray<TKey> TAsset::*InKeysMember, TArray<int32> TAsset::*InOffsetsMember,
			TArray<int32> TAsset::*InIndicesMember)
			: Name(InName)
			, KeysMember(InKeysMember)
			, OffsetsMember(InOffsetsMember)
			, IndicesMember(InIndicesMember)
		{
		}

		virtual const void* GetKeyTypeId() const override { return KeyTypeId<TKey>(); }

		virtual bool Bind(const UPrimaryDataAsset& Asset) override
		{
			const TAsset* TypedAsset = Cast<TAsset>(&Asset);
			if (!TypedAsset)
			{
				return false;
			}
			Keys = &(TypedAsset->*KeysMember);
			Offsets = &(TypedAsset->*OffsetsMember);
			Indices = &(TypedAsset->*IndicesMember);
			if (Offsets->Num() != Keys->Num() + 1 || Offsets->Last() != Indices->Num())
			{
				// The index is never rebuilt at runtime; a broken one is disabled instead.
				UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Invalid CSR index lengths: %s.%s"), *Asset.GetName(), *Name.ToString());
				Reset();
			}
			return true;
		}

		virtual void Reset() override
		{
			Keys = nullptr;
			Offsets = nullptr;
			Indices = nullptr;
		}

		virtual TConstArrayView<int32> Find(const void* Key) const override
		{
			if (!Keys || !Offsets || !Indices)
			{
				return {};
			}
			const TKey& TypedKey = *static_cast<const TKey*>(Key);
			const int32 Bucket = Algo::LowerBound(*Keys, TypedKey, KeyLess<TKey>);
			if (!Keys->IsValidIndex(Bucket) || (*Keys)[Bucket] != TypedKey || !Offsets->IsValidIndex(Bucket + 1))
			{
				return {};
			}
			const int32 Begin = (*Offsets)[Bucket];
			const int32 End = (*Offsets)[Bucket + 1];
			if (Begin < 0 || End < Begin || End > Indices->Num())
			{
				return {};
			}
			return MakeArrayView(*Indices).Slice(Begin, End - Begin);
		}

	private:
		FName Name;
		TArray<TKey> TAsset::*KeysMember;
		TArray<int32> TAsset::*OffsetsMember;
		TArray<int32> TAsset::*IndicesMember;
		const TArray<TKey>* Keys = nullptr;
		const TArray<int32>* Offsets = nullptr;
		const TArray<int32>* Indices = nullptr;
	};
	class FArrayPoolViewBase
	{
	public:
		virtual ~FArrayPoolViewBase() = default;
		virtual int32 GetElementSize() const = 0;
		virtual bool Bind(const UPrimaryDataAsset& Asset) = 0;
		virtual const void* GetData() const = 0;
		virtual int32 Num() const = 0;
		virtual void Reset() = 0;
	};

	/** The elements of one array field for every row, baked by the generator (<Field>_Pool). */
	template <typename TAsset, typename TElement>
	class TArrayPoolView final : public FArrayPoolViewBase
	{
	public:
		explicit TArrayPoolView(TArray<TElement> TAsset::*InPoolMember)
			: PoolMember(InPoolMember)
		{
		}

		virtual int32 GetElementSize() const override { return sizeof(TElement); }

		virtual bool Bind(const UPrimaryDataAsset& Asset) override
		{
			const TAsset* TypedAsset = Cast<TAsset>(&Asset);
			if (!TypedAsset)
			{
				return false;
			}
			Pool = &(TypedAsset->*PoolMember);
			return true;
		}

		virtual const void* GetData() const override { return Pool ? Pool->GetData() : nullptr; }
		virtual int32 Num() const override { return Pool ? Pool->Num() : 0; }
		virtual void Reset() override { Pool = nullptr; }

	private:
		TArray<TElement> TAsset::*PoolMember;
		const TArray<TElement>* Pool = nullptr;
	};
} // namespace DrTable::Private

/** Type-erased registered table. */
class DRTABLERUNTIME_API FDrTableRowTableBase
{
public:
	virtual ~FDrTableRowTableBase() = default;

	/** The row struct. Used as the table's type identity (stable across modules). */
	virtual const UScriptStruct* GetRowStruct() const = 0;
	virtual const UClass* GetAssetClass() const = 0;
	virtual bool Load(const UPrimaryDataAsset& Source) = 0;
	virtual void Reset() = 0;
	virtual int32 Num() const = 0;
	virtual bool IsLoaded() const = 0;

	const FString& GetExpectedSchemaHash() const { return ExpectedSchemaHash; }
	const FString& GetExpectedContentHash() const { return ExpectedContentHash; }

protected:
	FString ExpectedSchemaHash;
	FString ExpectedContentHash;
};

/**
 * A read-only view over the rows and indices of one baked table asset.
 *
 * Rows are never copied and no index is built at runtime: primary keys are a sorted
 * array searched with a binary search, sub keys are the CSR index baked by the tool.
 *
 * Lifetime: pointers and views returned from lookups point into the asset's arrays.
 * They stay valid until the table is reloaded (DrTable.Reload or re-baking in the
 * editor). Do not keep them across a reload; look rows up again instead.
 */
template <typename TRow>
class TDrTableRowTable final : public FDrTableRowTableBase
{
public:
	virtual const UScriptStruct* GetRowStruct() const override { return TRow::StaticStruct(); }
	virtual const UClass* GetAssetClass() const override { return AssetClass; }
	virtual int32 Num() const override { return Rows ? Rows->Num() : 0; }
	virtual bool IsLoaded() const override { return Rows != nullptr; }

	TDrTableRowTable& WithSchemaHash(const TCHAR* InSchemaHash)
	{
		ExpectedSchemaHash = InSchemaHash;
		return *this;
	}

	TDrTableRowTable& WithContentHash(const TCHAR* InContentHash)
	{
		ExpectedContentHash = InContentHash;
		return *this;
	}

	template <typename TAsset, typename TPrimaryKey>
	void Configure(TArray<TRow> TAsset::*InRowsMember, TArray<TPrimaryKey> TAsset::*InPrimaryKeysMember)
	{
		static_assert(std::is_base_of_v<UPrimaryDataAsset, TAsset>, "Table assets must derive from UPrimaryDataAsset.");
		AssetClass = TAsset::StaticClass();
		RowsBinder = [InRowsMember](const UPrimaryDataAsset& Asset)
		{
			const TAsset* TypedAsset = CastChecked<TAsset>(&Asset);
			return &(TypedAsset->*InRowsMember);
		};
		PrimaryKeyView = MakeUnique<DrTable::Private::TPrimaryKeyView<TAsset, TPrimaryKey>>(InPrimaryKeysMember);
	}

	template <typename TAsset, typename TKey>
	TDrTableRowTable& WithSubKey(FName Name, TArray<TKey> TAsset::*KeysMember, TArray<int32> TAsset::*OffsetsMember,
		TArray<int32> TAsset::*IndicesMember)
	{
		check(AssetClass == TAsset::StaticClass());
		SubKeyViews.Add(Name, MakeUnique<DrTable::Private::TSubKeyView<TAsset, TKey>>(Name, KeysMember, OffsetsMember, IndicesMember));
		return *this;
	}

	/** Same as WithSubKey. Kept as a readable marker for sub keys whose values are unique. */
	template <typename TAsset, typename TKey>
	TDrTableRowTable& WithUniqueSubKey(FName Name, TArray<TKey> TAsset::*KeysMember, TArray<int32> TAsset::*OffsetsMember,
		TArray<int32> TAsset::*IndicesMember)
	{
		return WithSubKey(Name, KeysMember, OffsetsMember, IndicesMember);
	}

	/** An array field: its elements for every row in the asset's pool; a row keeps Start and Num. */
	template <typename TAsset, typename TElement>
	TDrTableRowTable& WithArray(FName Name, TArray<TElement> TAsset::*PoolMember)
	{
		check(AssetClass == TAsset::StaticClass());
		ArrayViews.Add(Name, MakeUnique<DrTable::Private::TArrayPoolView<TAsset, TElement>>(PoolMember));
		return *this;
	}

	virtual bool Load(const UPrimaryDataAsset& Source) override
	{
		Reset();
		if (!AssetClass || !Source.IsA(AssetClass) || !RowsBinder || !PrimaryKeyView)
		{
			return false;
		}
		Rows = RowsBinder(Source);
		if (!Rows || !PrimaryKeyView->Bind(Source))
		{
			Reset();
			return false;
		}
		for (TPair<FName, TUniquePtr<DrTable::Private::FSubKeyViewBase>>& Pair : SubKeyViews)
		{
			if (!Pair.Value->Bind(Source))
			{
				Reset();
				return false;
			}
		}
		for (TPair<FName, TUniquePtr<DrTable::Private::FArrayPoolViewBase>>& Pair : ArrayViews)
		{
			if (!Pair.Value->Bind(Source))
			{
				Reset();
				return false;
			}
		}
		return true;
	}

	virtual void Reset() override
	{
		Rows = nullptr;
		if (PrimaryKeyView)
		{
			PrimaryKeyView->Reset();
		}
		for (TPair<FName, TUniquePtr<DrTable::Private::FSubKeyViewBase>>& Pair : SubKeyViews)
		{
			Pair.Value->Reset();
		}
		for (TPair<FName, TUniquePtr<DrTable::Private::FArrayPoolViewBase>>& Pair : ArrayViews)
		{
			Pair.Value->Reset();
		}
	}

	/** Looks a row up by its primary key written as text (e.g. "2001" or an enum name). */
	const TRow* FindRow(FName RowName) const
	{
		if (!Rows || !PrimaryKeyView)
		{
			return nullptr;
		}
		const int32 Index = PrimaryKeyView->Find(RowName);
		return Rows->IsValidIndex(Index) ? &(*Rows)[Index] : nullptr;
	}

	/** Looks a row up by its primary key. The key type must match the table's key type exactly. */
	template <typename TKey>
	const TRow* FindRowByKey(const TKey& Key) const
	{
		if (!Rows || !PrimaryKeyView || PrimaryKeyView->GetKeyTypeId() != DrTable::Private::KeyTypeId<TKey>())
		{
			return nullptr;
		}
		const int32 Index = PrimaryKeyView->FindTyped(&Key);
		return Rows->IsValidIndex(Index) ? &(*Rows)[Index] : nullptr;
	}

	TConstArrayView<TRow> GetRows() const { return Rows ? MakeArrayView(*Rows) : TConstArrayView<TRow>(); }

	/** Indices (into GetRows()) of the rows whose sub key equals Key. */
	template <typename TKey>
	TConstArrayView<int32> FindAllByKey(FName Name, const TKey& Key) const
	{
		const TUniquePtr<DrTable::Private::FSubKeyViewBase>* Found = SubKeyViews.Find(Name);
		if (!Found || (*Found)->GetKeyTypeId() != DrTable::Private::KeyTypeId<TKey>())
		{
			return {};
		}
		return (*Found)->Find(&Key);
	}

	/** Rows whose sub key equals Key. */
	template <typename TKey>
	TArray<const TRow*> FindRowsBySubKey(FName Name, const TKey& Key) const
	{
		TArray<const TRow*> Result;
		const TConstArrayView<TRow> AllRows = GetRows();
		for (const int32 Index : FindAllByKey(Name, Key))
		{
			if (AllRows.IsValidIndex(Index))
			{
				Result.Add(&AllRows[Index]);
			}
		}
		return Result;
	}

	/**
	 * Elements [Start, Start + Count) of an array field's pool (generated FMyRow::Get<Field>()).
	 * Empty when the field is unknown, the element type differs or the range is outside the pool.
	 */
	template <typename TElement>
	TConstArrayView<TElement> GetArray(FName Name, int32 Start, int32 Count) const
	{
		const TUniquePtr<DrTable::Private::FArrayPoolViewBase>* Found = ArrayViews.Find(Name);
		if (!Found || (*Found)->GetElementSize() != sizeof(TElement) || !(*Found)->GetData())
		{
			return {};
		}
		const int32 PoolNum = (*Found)->Num();
		if (Start < 0 || Count <= 0 || Start > PoolNum || Count > PoolNum - Start)
		{
			return {};
		}
		return TConstArrayView<TElement>(static_cast<const TElement*>((*Found)->GetData()) + Start, Count);
	}

private:
	const UClass* AssetClass = nullptr;
	TFunction<const TArray<TRow>*(const UPrimaryDataAsset&)> RowsBinder;
	const TArray<TRow>* Rows = nullptr;
	TUniquePtr<DrTable::Private::FPrimaryKeyViewBase> PrimaryKeyView;
	TMap<FName, TUniquePtr<DrTable::Private::FSubKeyViewBase>> SubKeyViews;
	TMap<FName, TUniquePtr<DrTable::Private::FArrayPoolViewBase>> ArrayViews;
};

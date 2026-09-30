// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Engine/DataAsset.h"
#include "Subsystems/EngineSubsystem.h"
#include "DrTableRowTable.h"
#include "DrTableRegistry.generated.h"

class UDrTableRegistry;

/** Signature of a generated registration function, e.g. MyGeneratedTables::RegisterAll<UDrTableRegistry>. */
using FDrTableRegisterFunction = void (*)(UDrTableRegistry&);

/**
 * Collects generated registration functions from game modules. Use through
 * DRTABLE_AUTO_REGISTER in one .cpp of the module that owns the generated code.
 */
struct DRTABLERUNTIME_API FDrTableAutoRegister
{
	explicit FDrTableAutoRegister(FDrTableRegisterFunction Function);
	static const TArray<FDrTableRegisterFunction>& GetFunctions();
};

/**
 * Registers the generated tables of a module with the global registry:
 *     #include "MyTableRegistration.h"
 *     DRTABLE_AUTO_REGISTER(MyGeneratedTables::RegisterAll<UDrTableRegistry>);
 */
#define DRTABLE_AUTO_REGISTER(Function) \
	static const FDrTableAutoRegister UE_JOIN(GDrTableAutoRegister_, __LINE__)(&Function)

/**
 * Owns the registered tables and loads their baked assets.
 *
 * The engine-wide instance lives in UDrTableSubsystem and is what the generated
 * lookups (FMyRow::Find, ...) use through UDrTableRegistry::Get(). Standalone
 * instances can be created with NewObject for tests or custom hosting.
 */
UCLASS()
class DRTABLERUNTIME_API UDrTableRegistry : public UObject
{
	GENERATED_BODY()

public:
	/** The active registry: the override if one is set, otherwise the engine subsystem's. Loads tables on first use if enabled. */
	static UDrTableRegistry* Get();

	/** Makes Get() return Registry (which may be nullptr) until ClearOverride(). Intended for tests and custom hosting. */
	static void SetOverride(UDrTableRegistry* Registry);

	/** Makes Get() return the engine subsystem's registry again. */
	static void ClearOverride();

	/** New registries start with AssetRoot and ExtraAssets copied from UDrTableSettings. */
	virtual void PostInitProperties() override;

	/** Runs every function collected by DRTABLE_AUTO_REGISTER. */
	void RegisterAutoTables();

	/** (Re)loads every registered table from <AssetRoot>/<TableId> plus the configured extra assets. */
	void LoadAllTables();
	void ReloadAllTables() { LoadAllTables(); }
	bool HasLoaded() const { return bHasLoaded; }

	/** Assets to load in addition to the automatic paths. Filled from UDrTableSettings::ExtraAssets on the global registry. */
	UPROPERTY(Transient)
	TArray<TSoftObjectPtr<UPrimaryDataAsset>> ExtraAssets;

	/** Folder of the automatic paths. Empty disables automatic paths (only ExtraAssets are loaded). */
	UPROPERTY(Transient)
	FString AssetRoot;

	/** Registers a table. Called by the generated RegisterAll. Registering the same id, row and asset class again returns the existing table. */
	template <typename TRow, typename TAsset, typename TPrimaryKey>
	TDrTableRowTable<TRow>& Register(FName TableId, TArray<TRow> TAsset::*RowsMember, TArray<TPrimaryKey> TAsset::*PrimaryKeysMember)
	{
		if (TUniquePtr<FDrTableRowTableBase>* Found = Tables.Find(TableId))
		{
			if ((*Found)->GetRowStruct() == TRow::StaticStruct() && (*Found)->GetAssetClass() == TAsset::StaticClass())
			{
				return *static_cast<TDrTableRowTable<TRow>*>(Found->Get());
			}
		}
		TUniquePtr<FDrTableRowTableBase> NewTable = MakeUnique<TDrTableRowTable<TRow>>();
		TDrTableRowTable<TRow>* Result = static_cast<TDrTableRowTable<TRow>*>(NewTable.Get());
		Result->template Configure<TAsset>(RowsMember, PrimaryKeysMember);
		Tables.Add(TableId, MoveTemp(NewTable));
		RebuildTypeIndex();
		return *Result;
	}

	/** Table registered for a row type, or nullptr if none or more than one is registered for it. */
	template <typename TRow>
	const TDrTableRowTable<TRow>* FindTable() const
	{
		return static_cast<const TDrTableRowTable<TRow>*>(FindTableByStruct(TRow::StaticStruct()));
	}

	template <typename TRow>
	const TDrTableRowTable<TRow>* FindTable(FName TableId) const
	{
		const TUniquePtr<FDrTableRowTableBase>* Found = Tables.Find(TableId);
		if (!Found || (*Found)->GetRowStruct() != TRow::StaticStruct())
		{
			return nullptr;
		}
		return static_cast<const TDrTableRowTable<TRow>*>(Found->Get());
	}

	template <typename TRow, typename TKey>
	const TRow* FindRowByKey(const TKey& Key) const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>();
		return Table ? Table->FindRowByKey(Key) : nullptr;
	}

	template <typename TRow, typename TKey>
	const TRow* FindRowByKey(FName TableId, const TKey& Key) const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>(TableId);
		return Table ? Table->FindRowByKey(Key) : nullptr;
	}

	template <typename TRow>
	const TRow* FindRow(FName TableId, FName RowName) const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>(TableId);
		return Table ? Table->FindRow(RowName) : nullptr;
	}

	template <typename TRow, typename TKey>
	TArray<const TRow*> FindRowsBySubKey(FName SubKeyName, const TKey& Key) const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>();
		return Table ? Table->FindRowsBySubKey(SubKeyName, Key) : TArray<const TRow*>();
	}

	template <typename TRow, typename TKey>
	TConstArrayView<int32> FindAllByKey(FName SubKeyName, const TKey& Key) const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>();
		return Table ? Table->FindAllByKey(SubKeyName, Key) : TConstArrayView<int32>();
	}

	template <typename TRow>
	TConstArrayView<TRow> GetRows() const
	{
		const TDrTableRowTable<TRow>* Table = FindTable<TRow>();
		return Table ? Table->GetRows() : TConstArrayView<TRow>();
	}

	/** One line per table: id, row struct, loaded row count. */
	void DumpStatus(FOutputDevice& Out) const;

private:
	const FDrTableRowTableBase* FindTableByStruct(const UScriptStruct* RowStruct) const;
	void RebuildTypeIndex();
	FDrTableRowTableBase* ResolveTable(const UPrimaryDataAsset& Source) const;

	/** Keeps the loaded assets alive; the table views point into their arrays. */
	UPROPERTY(Transient)
	TArray<TObjectPtr<UPrimaryDataAsset>> LoadedAssets;

	TMap<FName, TUniquePtr<FDrTableRowTableBase>> Tables;

	/** Row struct -> table. A struct registered more than once maps to nullptr (ambiguous). Rebuilt on Register. */
	TMap<const UScriptStruct*, FDrTableRowTableBase*> TablesByStruct;

	bool bHasLoaded = false;
};

/** Hosts the engine-wide registry. */
UCLASS()
class DRTABLERUNTIME_API UDrTableSubsystem : public UEngineSubsystem
{
	GENERATED_BODY()

public:
	virtual void Initialize(FSubsystemCollectionBase& Collection) override;
	virtual void Deinitialize() override;

	UDrTableRegistry* GetRegistry() const { return Registry; }

private:
	UPROPERTY(Transient)
	TObjectPtr<UDrTableRegistry> Registry;
};

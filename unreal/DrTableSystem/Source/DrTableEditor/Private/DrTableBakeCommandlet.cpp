// Copyright ldnjin81. All Rights Reserved.

#include "DrTableBakeCommandlet.h"

#include "Dom/JsonObject.h"
#include "Dom/JsonValue.h"
#include "HAL/FileManager.h"
#include "JsonObjectConverter.h"
#include "Misc/FileHelper.h"
#include "Misc/PackageName.h"
#include "Misc/Parse.h"
#include "Misc/Paths.h"
#include "Modules/ModuleManager.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "DrTableAssetBase.h"
#include "UObject/Package.h"
#include "UObject/SavePackage.h"
#include "UObject/UObjectGlobals.h"
#include "UObject/UnrealType.h"

DEFINE_LOG_CATEGORY_STATIC(LogDrTableBake, Log, All);

namespace DrTableBake
{
	bool LoadJsonObject(const FString& Filename, TSharedPtr<FJsonObject>& OutObject)
	{
		FString JsonText;
		if (!FFileHelper::LoadFileToString(JsonText, *Filename))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("Cannot read JSON file: %s"), *Filename);
			return false;
		}
		const TSharedRef<TJsonReader<>> Reader = TJsonReaderFactory<>::Create(JsonText);
		if (!FJsonSerializer::Deserialize(Reader, OutObject) || !OutObject.IsValid())
		{
			UE_LOG(LogDrTableBake, Error, TEXT("Invalid JSON: %s"), *Filename);
			return false;
		}
		return true;
	}

	bool SetArrayProperty(UObject* Asset, const FName PropertyName, const TArray<TSharedPtr<FJsonValue>>& Values, const FString& Table)
	{
		FArrayProperty* ArrayProperty = FindFProperty<FArrayProperty>(Asset->GetClass(), PropertyName);
		if (!ArrayProperty)
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Asset class has no array property '%s'."), *Table, *PropertyName.ToString());
			return false;
		}
		void* Value = ArrayProperty->ContainerPtrToValuePtr<void>(Asset);
		FText Failure;
		if (!FJsonObjectConverter::JsonValueToUProperty(MakeShared<FJsonValueArray>(Values), ArrayProperty, Value, 0, 0, true, &Failure))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Cannot convert property '%s': %s"), *Table, *PropertyName.ToString(), *Failure.ToString());
			return false;
		}
		return true;
	}

	UClass* FindTableClass(const FString& Prefix, const FString& Table)
	{
		// Reflected class names do not include the C++ 'U' prefix.
		const FString ClassName = FString::Printf(TEXT("%s%sTable"), *Prefix, *Table);
		UClass* Class = FindFirstObject<UClass>(*ClassName, EFindFirstObjectOptions::NativeFirst, ELogVerbosity::NoLogging, TEXT("DrTableBake"));
		if (!Class)
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Class U%s is not compiled into the editor. Build the project after `drtable build`, then bake."), *Table, *ClassName);
			return nullptr;
		}
		if (!Class->IsChildOf(UDrTableAssetBase::StaticClass()))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] U%s must derive from UDrTableAssetBase (generate with --ue-plugin)."), *Table, *ClassName);
			return nullptr;
		}
		return Class;
	}

	bool FillAsset(UDrTableAssetBase* Asset, const TSharedPtr<FJsonObject>& Payload, const FString& Table)
	{
		FString SchemaHash;
		FString ContentHash;
		if (!Payload->TryGetStringField(TEXT("schema_hash"), SchemaHash) || SchemaHash.IsEmpty())
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] schema_hash is missing."), *Table);
			return false;
		}
		Payload->TryGetStringField(TEXT("content_hash"), ContentHash);

		const TArray<TSharedPtr<FJsonValue>>* Rows = nullptr;
		const TArray<TSharedPtr<FJsonValue>>* PrimaryKeys = nullptr;
		const TArray<TSharedPtr<FJsonValue>>* SubKeys = nullptr;
		if (!Payload->TryGetArrayField(TEXT("rows"), Rows) || !Payload->TryGetArrayField(TEXT("primary_keys"), PrimaryKeys)
			|| !Payload->TryGetArrayField(TEXT("sub_keys"), SubKeys))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] rows, primary_keys or sub_keys is missing (bake the *client* JSON)."), *Table);
			return false;
		}
		if (Rows->Num() != PrimaryKeys->Num())
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] rows (%d) and primary_keys (%d) differ in length."), *Table, Rows->Num(), PrimaryKeys->Num());
			return false;
		}
		if (!SetArrayProperty(Asset, TEXT("Rows"), *Rows, Table) || !SetArrayProperty(Asset, TEXT("PrimaryKeys"), *PrimaryKeys, Table))
		{
			return false;
		}
		for (const TSharedPtr<FJsonValue>& SubKeyValue : *SubKeys)
		{
			const TSharedPtr<FJsonObject>* SubKey = nullptr;
			FString Field;
			const TArray<TSharedPtr<FJsonValue>>* Keys = nullptr;
			const TArray<TSharedPtr<FJsonValue>>* Offsets = nullptr;
			const TArray<TSharedPtr<FJsonValue>>* Indices = nullptr;
			if (!SubKeyValue.IsValid() || !SubKeyValue->TryGetObject(SubKey) || !SubKey || !SubKey->IsValid()
				|| !(*SubKey)->TryGetStringField(TEXT("field"), Field) || !(*SubKey)->TryGetArrayField(TEXT("keys"), Keys)
				|| !(*SubKey)->TryGetArrayField(TEXT("offsets"), Offsets) || !(*SubKey)->TryGetArrayField(TEXT("indices"), Indices))
			{
				UE_LOG(LogDrTableBake, Error, TEXT("[%s] A sub_keys entry is missing field/keys/offsets/indices."), *Table);
				return false;
			}
			if (!SetArrayProperty(Asset, FName(Field + TEXT("_Keys")), *Keys, Table)
				|| !SetArrayProperty(Asset, FName(Field + TEXT("_Offsets")), *Offsets, Table)
				|| !SetArrayProperty(Asset, FName(Field + TEXT("_Indices")), *Indices, Table))
			{
				return false;
			}
		}
		Asset->SchemaHash = SchemaHash;
		Asset->ContentHash = ContentHash;
		return true;
	}

	enum class EResult
	{
		Saved,
		UpToDate,
		Failed
	};

	EResult ProcessTable(const FString& OutputDirectory, const FString& AssetName, const FString& Table, UClass* Class,
		const TSharedPtr<FJsonObject>& Payload, bool bForce, bool bVerify)
	{
		const FString PackageName = OutputDirectory / AssetName;
		FString SchemaHash;
		FString ContentHash;
		Payload->TryGetStringField(TEXT("schema_hash"), SchemaHash);
		Payload->TryGetStringField(TEXT("content_hash"), ContentHash);

		UPackage* Package = FindPackage(nullptr, *PackageName);
		if (!Package && FPackageName::DoesPackageExist(PackageName))
		{
			Package = LoadPackage(nullptr, *PackageName, LOAD_None);
		}
		UObject* ExistingObject = Package ? FindObject<UObject>(Package, *AssetName) : nullptr;
		if (ExistingObject && ExistingObject->GetClass() != Class)
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Existing asset class %s differs from generated class %s. Delete or rename %s."),
				*Table, *ExistingObject->GetClass()->GetName(), *Class->GetName(), *PackageName);
			return EResult::Failed;
		}
		UDrTableAssetBase* Existing = Cast<UDrTableAssetBase>(ExistingObject);
		const bool bUpToDate = Existing && Existing->SchemaHash == SchemaHash && Existing->ContentHash == ContentHash;

		if (bVerify)
		{
			if (!Existing)
			{
				UE_LOG(LogDrTableBake, Error, TEXT("[%s] Not baked: %s"), *Table, *PackageName);
				return EResult::Failed;
			}
			if (!bUpToDate)
			{
				UE_LOG(LogDrTableBake, Error, TEXT("[%s] Out of date: %s (schema %s, content %s)"), *Table, *PackageName,
					Existing->SchemaHash == SchemaHash ? TEXT("ok") : TEXT("differs"), Existing->ContentHash == ContentHash ? TEXT("ok") : TEXT("differs"));
				return EResult::Failed;
			}
			UE_LOG(LogDrTableBake, Display, TEXT("[%s] Up to date."), *Table);
			return EResult::UpToDate;
		}
		if (bUpToDate && !bForce)
		{
			UE_LOG(LogDrTableBake, Display, TEXT("[%s] Up to date, skipped."), *Table);
			return EResult::UpToDate;
		}

		if (!Package)
		{
			Package = CreatePackage(*PackageName);
		}
		UDrTableAssetBase* Asset = Existing ? Existing : NewObject<UDrTableAssetBase>(Package, Class, *AssetName, RF_Public | RF_Standalone);
		if (!Asset)
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Cannot create the asset."), *Table);
			return EResult::Failed;
		}
		Asset->Modify();
		if (!FillAsset(Asset, Payload, Table))
		{
			return EResult::Failed;
		}
		Asset->PostEditChange();
		Package->MarkPackageDirty();

		const FString Filename = FPackageName::LongPackageNameToFilename(PackageName, FPackageName::GetAssetPackageExtension());
		IFileManager::Get().MakeDirectory(*FPaths::GetPath(Filename), true);
		FSavePackageArgs SaveArgs;
		SaveArgs.TopLevelFlags = RF_Public | RF_Standalone;
		if (!UPackage::SavePackage(Package, Asset, *Filename, SaveArgs))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] Cannot save: %s"), *Table, *Filename);
			return EResult::Failed;
		}
		UE_LOG(LogDrTableBake, Display, TEXT("[%s] Saved %s"), *Table, *PackageName);
		return EResult::Saved;
	}
} // namespace DrTableBake

UDrTableBakeCommandlet::UDrTableBakeCommandlet()
{
	IsClient = false;
	IsEditor = true;
	IsServer = false;
	LogToConsole = true;
	ShowErrorCount = true;
	UseCommandletResultAsExitCode = true;
	HelpDescription = TEXT("Bakes DrTableSystem client JSON into DataAssets. -Input=<dir> [-Out=/Game/Data] [-Force] [-Verify]");
}

int32 UDrTableBakeCommandlet::Main(const FString& Params)
{
	FString InputDirectory;
	FString OutputDirectory = TEXT("/Game/Data");
	if (!FParse::Value(*Params, TEXT("Input="), InputDirectory))
	{
		UE_LOG(LogDrTableBake, Error, TEXT("Usage: -run=DrTableBake -Input=<client json dir> [-Out=/Game/Data] [-Force] [-Verify]"));
		return 1;
	}
	FParse::Value(*Params, TEXT("Out="), OutputDirectory);
	const bool bForce = FParse::Param(*Params, TEXT("Force"));
	const bool bVerify = FParse::Param(*Params, TEXT("Verify"));

	InputDirectory = FPaths::ConvertRelativePathToFull(InputDirectory);
	FPaths::NormalizeDirectoryName(InputDirectory);
	FPaths::NormalizeDirectoryName(OutputDirectory);
	OutputDirectory.RemoveFromEnd(TEXT("/"));
	if (!IFileManager::Get().DirectoryExists(*InputDirectory))
	{
		UE_LOG(LogDrTableBake, Error, TEXT("Input directory not found: %s"), *InputDirectory);
		return 1;
	}
	FText InvalidReason;
	if (!FPackageName::IsValidLongPackageName(OutputDirectory, true, &InvalidReason))
	{
		UE_LOG(LogDrTableBake, Error, TEXT("-Out must be a long package path such as /Game/Data: %s (%s)"), *OutputDirectory, *InvalidReason.ToString());
		return 1;
	}

	TSharedPtr<FJsonObject> Manifest;
	if (!DrTableBake::LoadJsonObject(FPaths::Combine(InputDirectory, TEXT("manifest.json")), Manifest))
	{
		return 1;
	}
	FString Prefix;
	FString AssetNameFormat = TEXT("DA_{table}");
	if (!Manifest->TryGetStringField(TEXT("cpp_prefix"), Prefix))
	{
		UE_LOG(LogDrTableBake, Error, TEXT("manifest.json has no cpp_prefix. Regenerate with a current drtable."));
		return 1;
	}
	Manifest->TryGetStringField(TEXT("asset_name"), AssetNameFormat);
	const TArray<TSharedPtr<FJsonValue>>* Tables = nullptr;
	if (!Manifest->TryGetArrayField(TEXT("tables"), Tables))
	{
		UE_LOG(LogDrTableBake, Error, TEXT("manifest.json has no tables array."));
		return 1;
	}

	int32 Saved = 0;
	int32 UpToDate = 0;
	int32 Failed = 0;
	for (const TSharedPtr<FJsonValue>& Entry : *Tables)
	{
		const TSharedPtr<FJsonObject>* TableObject = nullptr;
		FString Table;
		if (!Entry.IsValid() || !Entry->TryGetObject(TableObject) || !TableObject || !(*TableObject)->TryGetStringField(TEXT("name"), Table))
		{
			UE_LOG(LogDrTableBake, Error, TEXT("A manifest table entry has no name."));
			++Failed;
			continue;
		}
		UClass* Class = DrTableBake::FindTableClass(Prefix, Table);
		TSharedPtr<FJsonObject> Payload;
		if (!Class || !DrTableBake::LoadJsonObject(FPaths::Combine(InputDirectory, Table + TEXT(".json")), Payload))
		{
			++Failed;
			continue;
		}
		FString PayloadTable;
		if (!Payload->TryGetStringField(TEXT("table"), PayloadTable) || PayloadTable != Table)
		{
			UE_LOG(LogDrTableBake, Error, TEXT("[%s] The JSON 'table' value '%s' does not match the manifest."), *Table, *PayloadTable);
			++Failed;
			continue;
		}
		const FString AssetName = AssetNameFormat.Replace(TEXT("{table}"), *Table);
		switch (DrTableBake::ProcessTable(OutputDirectory, AssetName, Table, Class, Payload, bForce, bVerify))
		{
		case DrTableBake::EResult::Saved: ++Saved; break;
		case DrTableBake::EResult::UpToDate: ++UpToDate; break;
		default: ++Failed; break;
		}
	}

	UE_LOG(LogDrTableBake, Display, TEXT("DrTable %s: saved %d, up to date %d, failed %d"), bVerify ? TEXT("verify") : TEXT("bake"), Saved, UpToDate, Failed);
	return Failed == 0 ? 0 : 1;
}

// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "UObject/ICookInfo.h"

class UDrTableSettings;

namespace DrTableCook
{
	void GatherPackages(const UDrTableSettings& Settings, TArray<FName>& OutPackages);
	void ModifyCook(UE::Cook::ICookInfo& Info, TArray<UE::Cook::FPackageCookRule>& Rules);
	bool IsValidAssetPath(const FSoftObjectPath& Path);
	bool ValidatePaths(const UObject& Asset, const FString& Table);
}

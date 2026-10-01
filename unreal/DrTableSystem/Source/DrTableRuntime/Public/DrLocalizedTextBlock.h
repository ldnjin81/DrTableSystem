// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Components/TextBlock.h"
#include "DrLocalizedTextBlock.generated.h"

/**
 * A text block that shows a string table entry and redraws itself when the language changes.
 * Set Table and Key in the designer, or call SetStringKey.
 */
UCLASS()
class DRTABLERUNTIME_API UDrLocalizedTextBlock : public UTextBlock
{
	GENERATED_BODY()

public:
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "DrTable")
	FName Table;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "DrTable")
	FName Key;

	UFUNCTION(BlueprintCallable, Category = "DrTable")
	void SetStringKey(FName InTable, FName InKey);

	/** Shows the current text of Table/Key again. */
	UFUNCTION(BlueprintCallable, Category = "DrTable")
	void RefreshText();

	virtual void SynchronizeProperties() override;
	virtual void ReleaseSlateResources(bool bReleaseChildren) override;

private:
	void HandleLanguageChanged(const FString& Language);
	void Bind();
	void Unbind();

	TWeakObjectPtr<class UDrStringTables> BoundTables;
	FDelegateHandle LanguageChangedHandle;
};
